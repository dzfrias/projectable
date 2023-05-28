#![allow(unused_imports)]

use crate::{
    app::{component::*, InputOperation, PendingOperation},
    config::Config,
    dir::*,
    external_event::{ExternalEvent, RefreshData},
    filelisting::{self, FileListing},
    ignore::{Ignore, IgnoreBuilder},
    queue::{AppEvent, Queue},
};
use anyhow::{anyhow, bail, Context, Result};
use crossterm::event::Event;
use easy_switch::switch;
use git2::{Repository, Status};
use ignore::Walk;
use itertools::Itertools;
use log::{debug, info, warn};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

pub struct Filetree {
    state: Cell<TreeState>,
    is_focused: bool,
    dir: Dir,
    listing: FileListing,
    root_path: PathBuf,
    queue: Queue,
    only_included: Vec<PathBuf>,
    repo: Option<Repository>,
    status_cache: Option<HashMap<PathBuf, Status>>,
    config: Rc<Config>,
    ignore: Ignore,
    #[allow(dead_code)]
    marks: Rc<RefCell<Vec<PathBuf>>>,
}

impl Filetree {
    fn from_dir(path: impl AsRef<Path>, queue: Queue) -> Result<Self> {
        let tree = DirBuilder::new(path.as_ref())
            .dirs_first(true)
            .build()
            .context("failed to create `Dir` when creating Filetree")?;
        let mut state = TreeState::default();
        state.select_first();
        let mut tree = Filetree {
            root_path: path.as_ref().to_path_buf(),
            state: state.into(),
            is_focused: true,
            queue: queue.clone(),
            dir: tree,
            only_included: Vec::new(),
            repo: Repository::open(path.as_ref().join(".git")).ok(),
            status_cache: None,
            config: Rc::new(Config::default()),
            ignore: Ignore::default(),
            marks: Default::default(),
            listing: FileListing::new(
                &Walk::new(path.as_ref())
                    .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
                    .filter(|entry_path| entry_path != path.as_ref())
                    .collect_vec(),
            ),
        };
        tree.populate_status_cache();
        if let Some(item) = tree.get_selected() {
            queue.add(AppEvent::PreviewFile(item.path().to_owned()));
        }
        tree.listing.fold_all();
        Ok(tree)
    }

    pub fn from_dir_with_config(
        path: impl AsRef<Path>,
        queue: Queue,
        config: Rc<Config>,
        marks: Rc<RefCell<Vec<PathBuf>>>,
    ) -> Result<Self> {
        let ignore = IgnoreBuilder::new(path.as_ref())
            .ignore(&config.filetree.ignore)
            .use_gitignore(config.filetree.use_gitignore)
            .build()
            .context("failed to create glob ignorer")?;
        let tree = DirBuilder::new(path.as_ref())
            .dirs_first(config.filetree.dirs_first)
            .ignore(&ignore)
            .build()
            .context("failed to build `Dir`")?;
        Ok(Filetree {
            repo: if config.filetree.use_git {
                Repository::open(path.as_ref().join(".git")).ok()
            } else {
                None
            },
            ignore,
            dir: tree,
            config: Rc::clone(&config),
            marks,
            ..Self::from_dir(path, queue)?
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path)
            .dirs_first(self.config.filetree.dirs_first)
            .ignore(&self.ignore)
            .build()
            .context("failed to build `Dir`")?;
        self.dir = tree;
        self.only_included = Vec::new();
        self.populate_status_cache();

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        Ok(())
    }

    pub fn partial_refresh(&mut self, refresh_data: &RefreshData) -> Result<()> {
        match refresh_data {
            RefreshData::Delete(path) => {
                if self.ignore.is_ignored(path) {
                    return Ok(());
                }

                self.listing.remove(path.as_path())?;
            }
            RefreshData::Add(path) => {
                if self.ignore.is_ignored(path) {
                    return Ok(());
                }

                if path.is_dir() {
                    self.listing.add(filelisting::Item::Dir(path.clone()));
                } else {
                    self.listing.add(filelisting::Item::File(path.clone()));
                }
                self.populate_status_cache();
            }
        }

        Ok(())
    }

    pub fn get_selected(&self) -> Option<&Item> {
        let state = self.state.take();
        let item = self.dir.nested_child(&state.selected())?;
        self.state.set(state);
        Some(item)
    }

    pub fn is_searching(&self) -> bool {
        !self.only_included.is_empty()
    }

    pub fn only_include(&mut self, include: Vec<PathBuf>) -> Result<()> {
        self.dir = DirBuilder::new(&self.root_path)
            .dirs_first(true)
            .ignore(&self.ignore)
            .only_include(&include)
            .build()
            .with_context(|| {
                format!("failed to build `Dir` while only-including files: \"{include:?}\"")
            })?;
        self.only_included = include;

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        if let Some(selected) = self.get_selected() {
            self.queue
                .add(AppEvent::PreviewFile(selected.path().to_owned()));
        }
        Ok(())
    }

    fn populate_status_cache(&mut self) {
        self.status_cache = self.repo.as_ref().and_then(|repo| {
            repo.statuses(None).ok().map(|statuses| {
                statuses
                    .iter()
                    .map(|status| (self.root_path.join(status.path().unwrap()), status.status()))
                    .collect::<HashMap<PathBuf, Status>>()
            })
        });
    }

    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let mut location = self
            .dir
            .location_by_path(&path)
            .ok_or(anyhow!("path not found"))?;
        if location.is_empty() {
            return Ok(());
        }
        self.state.get_mut().select(location.as_ref());
        while !location.is_empty() {
            let next_location = location
                .split_last()
                .expect("location should not be empty")
                .1
                .to_vec();
            self.state.get_mut().open(location);
            location = next_location;
        }
        self.queue
            .add(AppEvent::PreviewFile(path.as_ref().to_path_buf()));
        Ok(())
    }

    pub fn open_all(&mut self) {
        for item in self.dir.walk().filter(|item| matches!(item, Item::Dir(_))) {
            let loc = self
                .dir
                .location_by_path(item.path())
                .expect("item should be in tree");
            self.state.get_mut().open(loc);
        }
    }

    pub fn is_ignored(&self, path: impl AsRef<Path>) -> bool {
        self.ignore.is_ignored(path)
    }

    pub fn open_under(&mut self, location: &mut Vec<usize>) {
        let Item::Dir(dir) = self.dir.nested_child_mut(location).unwrap() else {
            return;
        };
        for index in 0..dir.iter().len() {
            location.push(index);
            self.state.get_mut().open(location.clone());
            self.open_under(location);
            location.pop();
        }
    }
}

impl Drawable for Filetree {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let mut state = ListState::default();
        state.select(Some(self.listing.selected()));
        let list = List::new(
            self.listing
                .items()
                .into_iter()
                .map(|item| {
                    let file_name = item
                        .path()
                        .file_name()
                        .expect("path should have name")
                        .to_string_lossy();
                    // Calculate depth of indent
                    let indent_amount =
                        item.path().components().count() - self.listing.root().components().count();
                    const INDENT: usize = 2;

                    ListItem::new(format!("{}{file_name}", " ".repeat(indent_amount * INDENT)))
                })
                .collect_vec(),
        )
        .highlight_style(Style::default().bg(Color::Red))
        .block(Block::default().borders(Borders::ALL));
        f.render_stateful_widget(list, area, &mut state);

        Ok(())

        // let items = build_filetree(
        //     &self.dir,
        //     self.status_cache.as_ref(),
        //     Rc::clone(&self.config),
        //     &self.marks.borrow(),
        //     &self.only_included,
        // );
        // let mut state = self.state.take();
        //
        // if self.is_searching() {
        //     let layout = Layout::default()
        //         .constraints([
        //             Constraint::Length(1),
        //             Constraint::Length(1),
        //             Constraint::Min(1),
        //         ])
        //         .margin(1)
        //         .split(area);
        //     let block = Block::default()
        //         .borders(Borders::ALL)
        //         .border_style(self.config.filetree.border_color.into())
        //         .title("Files");
        //     let tree = Tree::new(items).highlight_style(self.config.selected.into());
        //     let p = Paragraph::new("Some results may be filtered out ('\\' to reset)")
        //         .alignment(Alignment::Center)
        //         .wrap(Wrap { trim: true })
        //         .style(self.config.filetree.filtered_out_message.into());
        //
        //     f.render_widget(block, area);
        //     f.render_widget(p, layout[0]);
        //     f.render_stateful_widget(tree, layout[2], &mut state);
        // } else {
        //     let tree = Tree::new(items)
        //         .block(
        //             Block::default()
        //                 .borders(Borders::ALL)
        //                 .title("Files")
        //                 .border_style(self.config.filetree.border_color.into()),
        //         )
        //         .highlight_style(self.config.selected.into());
        //     f.render_stateful_widget(tree, area, &mut state);
        // }
        //
        // self.state.set(state);
        //
        // Ok(())
    }
}

impl Component for Filetree {
    fn visible(&self) -> bool {
        true
    }

    fn focus(&mut self, focus: bool) {
        self.is_focused = focus;
    }
    fn focused(&self) -> bool {
        self.is_focused
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.focused() {
            return Ok(());
        }

        let items = build_filetree(&self.dir, None, Rc::clone(&self.config), &[], &[]);

        const JUMP_DOWN_AMOUNT: u8 = 3;
        match ev {
            ExternalEvent::RefreshFiletree => self.refresh().context("problem refreshing tree")?,
            ExternalEvent::PartialRefresh(data) => {
                for refresh_data in data {
                    if let Err(err) = self.partial_refresh(refresh_data).with_context(|| {
                        format!("problem partially refreshing tree with data: \"{data:?}\"")
                    }) {
                        // Caused by weird fsevent bug, see https://github.com/notify-rs/notify/issues/272
                        // for more info.
                        if err.root_cause().to_string() == "invalid remove target" {
                            debug!("swallowed invalid remove target error");
                            continue;
                        }
                        bail!(err)
                    };
                }
            }
            ExternalEvent::Crossterm(Event::Key(key)) => {
                let mut refresh_preview = true;
                let not_empty = !items.is_empty();
                switch! { key;
                    self.config.all_up => self.listing.select_first(),
                    self.config.all_down => self.listing.select_last(),
                    self.config.down, not_empty => self.listing.select_next(),
                    self.config.up, not_empty => self.listing.select_prev(),
                    self.config.filetree.down_three, not_empty => self.listing.select_next_n(JUMP_DOWN_AMOUNT as usize),
                    self.config.filetree.up_three, not_empty => self.listing.select_prev_n(JUMP_DOWN_AMOUNT as usize),
                    self.config.filetree.exec_cmd, not_empty => {
                        if let Some(item) = self.get_selected() {
                             self.queue.add(AppEvent::OpenInput(InputOperation::Command {
                                 to: item.path().to_path_buf(),
                             }));
                         }
                    },
                    self.config.filetree.delete => {
                        let item = self.listing.selected_item();
                        self.queue.add(AppEvent::OpenPopup(PendingOperation::DeleteFile(item.path().to_path_buf())));
                    },
                    self.config.filetree.diff_mode => self.queue.add(AppEvent::TogglePreviewMode),
                    self.config.filetree.git_filter => {
                        if let Some(cache) = self.status_cache.as_ref() {
                            info!("filtered for modified files");
                            self.only_include(cache.keys().cloned().collect_vec())?;
                        } else {
                            warn!("no git status to filter for");
                        }
                    },
                    self.config.filetree.search => self
                        .queue
                        .add(AppEvent::OpenInput(InputOperation::SearchFiles)),
                    self.config.filetree.clear => {
                        info!("refreshed filetree");
                        self.refresh().context("problem refreshing filetree")?;
                    },
                    self.config.open => match self.get_selected() {
                        Some(Item::Dir(_)) => self.listing.toggle_fold(),
                        Some(Item::File(file)) => self
                            .queue
                            .add(AppEvent::OpenFile(file.path().to_path_buf())),
                        None => {}
                    },
                    self.config.filetree.new_file => {
                        let is_folded = self.listing.is_folded(self.listing.selected()).unwrap();
                        let add_path = match self.listing.selected_item() {
                            filelisting::Item::Dir(dir) if !is_folded => dir,
                            item => item.path().parent().expect("item should have parent"),
                        };
                        self.queue
                            .add(AppEvent::OpenInput(InputOperation::NewFile { at: add_path.to_path_buf() }));
                    },
                    self.config.filetree.new_dir => {
                        let is_folded = self.listing.is_folded(self.listing.selected()).unwrap();
                        let add_path = match self.listing.selected_item() {
                            filelisting::Item::Dir(dir) if !is_folded => dir,
                            item => item.path().parent().expect("item should have parent"),
                        };
                        self.queue
                            .add(AppEvent::OpenInput(InputOperation::NewDir { at: add_path.to_path_buf() }));
                    },
                    self.config.filetree.close_all => self.state.get_mut().close_all(),
                    self.config.filetree.open_all => self.open_all(),
                    self.config.filetree.special_command => {
                        if let Some(selected) = self.get_selected() {
                            self.queue.add(AppEvent::SpecialCommand(selected.path().to_path_buf()));
                        }
                    },
                    self.config.filetree.mark_selected => {
                        if let Some(selected) = self.get_selected() {
                            self.queue.add(AppEvent::Mark(selected.path().to_path_buf()));
                        }
                    },
                    _ => refresh_preview = false,
                }
                if !refresh_preview {
                    return Ok(());
                }
                if let Some(item) = self.get_selected() {
                    self.queue
                        .add(AppEvent::PreviewFile(item.path().to_owned()));
                }
            }
            _ => {}
        }

        Ok(())
    }
}

fn last_of_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .iter()
        .last()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn build_filetree<'a>(
    tree: &'a Dir,
    statuses: Option<&HashMap<PathBuf, Status>>,
    config: Rc<Config>,
    marks: &[PathBuf],
    highlight: &[PathBuf],
) -> Vec<TreeItem<'a>> {
    let mut items = Vec::new();
    for item in tree {
        let style = 'style: {
            if highlight.iter().any(|path| path == item.path()) {
                break 'style config.filetree.searched_style.into();
            }
            if marks.iter().any(|path| path == item.path()) {
                break 'style config.filetree.marks_style.into();
            }
            statuses.map_or(Style::default(), |statuses| {
                statuses
                    .get(item.path())
                    .map_or(Style::default(), |status| match *status {
                        Status::WT_NEW => Style::from(config.filetree.git_new_style),
                        Status::WT_MODIFIED => Style::from(config.filetree.git_modified_style),
                        Status::INDEX_MODIFIED | Status::INDEX_NEW => {
                            Style::from(config.filetree.git_modified_style)
                        }
                        _ => Style::default(),
                    })
            })
        };
        let tree_item = match item {
            Item::Dir(dir) => TreeItem::new(
                last_of_path(dir.path()),
                build_filetree(dir, statuses, Rc::clone(&config), marks, highlight),
            )
            .style(style),
            Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())).style(style),
        };
        items.push(tree_item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;
    use test_log::test;

    #[test]
    fn last_of_path_only_gets_last_part() {
        let name = last_of_path("t/d/d/s/test.txt");
        assert_eq!("test.txt".to_owned(), name);
    }

    #[test]
    fn last_of_path_works_with_one_part() {
        let name = last_of_path("test.txt");
        assert_eq!("test.txt", name);
    }

    #[test]
    fn new_filetree_selects_first() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        assert_eq!(
            path.join("test.txt"),
            filetree.get_selected().unwrap().path()
        );
    }

    #[test]
    fn sends_delete_event() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let d = input_event!(KeyCode::Char('d'));
        filetree
            .handle_event(&d)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenPopup(PendingOperation::DeleteFile(
                path.join("test.txt")
            ))));
    }

    #[test]
    fn sends_new_file_and_new_dir_events() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile {
                at: path.clone()
            })));

        let caps_n = input_event!(KeyCode::Char('N'); KeyModifiers::SHIFT);
        filetree
            .handle_event(&caps_n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewDir { at: path })));
    }

    #[test]
    fn makes_new_file_as_sibling_when_selected_dir_is_closed() {
        let temp = temp_files!("test/test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        assert_eq!(path.join("test"), filetree.listing.selected_item().path());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile { at: path })));
    }

    #[test]
    fn makes_new_file_as_child_when_selected_dir_is_open() {
        let temp = temp_files!("test/test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        // Opens dir
        filetree.listing.toggle_fold();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let n = input_event!(KeyCode::Char('n'));
        filetree
            .handle_event(&n)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::NewFile {
                at: path.join("test")
            })));
    }

    #[test]
    fn enter_opens_when_over_dir() {
        let temp = temp_files!("test/test.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        filetree.listing.toggle_fold();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let enter = input_event!(KeyCode::Enter);
        filetree
            .handle_event(&enter)
            .expect("should be able to handle keypress");
        assert!(filetree
            .listing
            .is_folded(filetree.listing.selected())
            .unwrap());
    }

    #[test]
    fn enter_sends_open_file_when_over_files() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_owned();
        let mut filetree =
            Filetree::from_dir(&path, Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let enter = input_event!(KeyCode::Enter);
        filetree
            .handle_event(&enter)
            .expect("should be able to handle keypress");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenFile(path.join("test.txt"))));
    }

    #[test]
    fn can_jump_down_by_three() {
        let temp = temp_files!("test.txt", "test2.txt", "test3.txt", "test4.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let ctrl_n = input_event!(KeyCode::Char('n'); KeyModifiers::CONTROL);
        filetree
            .handle_event(&ctrl_n)
            .expect("should be able to handle keypress");
        assert_eq!(3, filetree.listing.selected());
    }

    #[test]
    fn can_jump_up_by_three() {
        let temp = temp_files!("test.txt", "test2.txt", "test3.txt", "test4.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let inputs = input_events!(KeyCode::Char('G'); KeyModifiers::SHIFT, KeyCode::Char('p'); KeyModifiers::CONTROL);
        for input in inputs {
            filetree
                .handle_event(&input)
                .expect("should be able to handle keypress");
        }
        assert_eq!(0, filetree.state.get_mut().selected()[0]);
    }

    #[test]
    fn can_send_run_cmd() {
        let temp = temp_files!("test.txt");
        let mut filetree =
            Filetree::from_dir(temp.path(), Queue::new()).expect("should be able to make filetree");
        let path = temp.to_path_buf();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let e = input_event!(KeyCode::Char('e'));
        filetree
            .handle_event(&e)
            .expect("should be able to handle event");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::Command {
                to: path.join("test.txt")
            })));
    }

    #[test]
    fn can_send_search_cmd() {
        let temp = temp_files!();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let slash = input_event!(KeyCode::Char('/'));
        filetree
            .handle_event(&slash)
            .expect("should be able to handle event");
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::SearchFiles)));
    }

    #[test]
    fn can_only_include() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();

        assert!(filetree
            .only_include(vec![temp.path().join("test.txt")])
            .is_ok());
        assert_eq!(1, filetree.dir.iter().len());
        temp.close().unwrap();
    }

    #[test]
    fn can_send_toggle_preview_cmd() {
        let temp = temp_files!();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let slash = input_event!(KeyCode::Char('t'));
        filetree
            .handle_event(&slash)
            .expect("should be able to handle event");
        assert!(filetree.queue.contains(&AppEvent::TogglePreviewMode));
    }

    #[test]
    fn can_open_path() {
        let temp = temp_files!("test/test.txt");
        let path = temp.path().to_owned();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert!(filetree.open_path(path.join("test")).is_ok());
        assert_eq!(1, filetree.state.get_mut().get_all_opened().len());
    }

    #[test]
    fn partial_refresh_delete_goes_to_same_item() {
        let temp = temp_files!("test/test.txt", "test/test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.listing.unfold_all();
        filetree.listing.select(1);
        filetree
            .partial_refresh(&RefreshData::Delete(
                filetree.listing.selected_item().path().to_path_buf(),
            ))
            .unwrap();
        assert_eq!(1, filetree.listing.selected());
    }

    #[test]
    fn partial_refresh_goes_to_parent_if_only_child() {
        let temp = temp_files!("test/test.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.listing.unfold_all();
        filetree.listing.select(1);
        filetree
            .partial_refresh(&RefreshData::Delete(
                filetree.listing.selected_item().path().to_path_buf(),
            ))
            .unwrap();
        assert_eq!(0, filetree.listing.selected());
    }

    #[test]
    fn can_open_all() {
        let temp = temp_files!("test.txt", "test/test2.txt", "test2/test4/test.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.open_all();
        assert_eq!(3, filetree.state.get_mut().get_all_opened().len());
    }

    #[test]
    fn can_mark_selected() {
        let temp = temp_files!("test.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        let path = temp.path().to_path_buf();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        let event = input_event!(KeyCode::Char('m'));
        assert!(filetree.handle_event(&event).is_ok());
        assert!(filetree
            .queue
            .contains(&AppEvent::Mark(path.join("test.txt"))));
    }

    #[test]
    fn swallow_invalid_delete_external_events() {
        let temp = temp_files!("test.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        let event =
            ExternalEvent::PartialRefresh(vec![RefreshData::Delete("does_not_exist.txt".into())]);
        assert!(filetree.handle_event(&event).is_ok());
    }

    #[test]
    fn opening_path_adds_preview_event_to_queue() {
        let temp = temp_files!("test.txt");
        let path = temp.path().to_path_buf();
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.open_path(path.join("test.txt")).unwrap();

        assert!(filetree
            .queue
            .contains(&AppEvent::PreviewFile(path.join("test.txt"))));
    }
}
