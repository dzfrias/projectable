use crate::{
    app::{component::*, InputOperation, PendingOperation},
    config::Config,
    external_event::{ExternalEvent, RefreshData},
    filelisting::{self, FileListing},
    queue::{AppEvent, Queue},
};
use anyhow::{bail, Context, Result};
use crossterm::event::Event;
use easy_switch::switch;
use git2::{Repository, Status};
use ignore::{
    overrides::{Override, OverrideBuilder},
    Walk, WalkBuilder,
};
use itertools::Itertools;
use log::{debug, info, warn};
use std::{
    cell::RefCell,
    collections::HashMap,
    iter,
    path::{Path, PathBuf},
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub struct Filetree {
    is_focused: bool,
    listing: FileListing,
    root_path: PathBuf,
    queue: Queue,
    repo: Option<Repository>,
    status_cache: Option<HashMap<PathBuf, Status>>,
    config: Rc<Config>,
    #[allow(dead_code)]
    marks: Rc<RefCell<Vec<PathBuf>>>,
}

impl Filetree {
    fn from_dir(path: impl AsRef<Path>, queue: Queue) -> Result<Self> {
        let mut tree = Filetree {
            root_path: path.as_ref().to_path_buf(),
            is_focused: true,
            queue: queue.clone(),
            repo: Repository::open(path.as_ref().join(".git")).ok(),
            status_cache: None,
            config: Rc::new(Config::default()),
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
        let overrides = build_override_ignorer(&path, &config.filetree.ignore)?;
        let mut listing = FileListing::new(
            &WalkBuilder::new(path.as_ref())
                .overrides(overrides)
                .build()
                .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
                .filter(|entry_path| entry_path != path.as_ref()) // Ignore root
                .collect_vec(),
        );
        listing.fold_all();

        Ok(Filetree {
            repo: if config.filetree.use_git {
                Repository::open(path.as_ref().join(".git")).ok()
            } else {
                None
            },
            listing,
            config: Rc::clone(&config),
            marks,
            ..Self::from_dir(path, queue)?
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let overrides = build_override_ignorer(&self.root_path, &self.config.filetree.ignore)?;
        let mut listing = FileListing::new(
            &WalkBuilder::new(&self.root_path)
                .overrides(overrides)
                .build()
                .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
                .filter(|entry_path| entry_path != &self.root_path) // Ignore root
                .collect_vec(),
        );
        listing.fold_all();
        self.listing = listing;
        self.populate_status_cache();

        Ok(())
    }

    pub fn partial_refresh(&mut self, refresh_data: &RefreshData) -> Result<()> {
        match refresh_data {
            RefreshData::Delete(path) => {
                self.listing.remove(path.as_path())?;
            }
            RefreshData::Add(path) => {
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

    pub fn get_selected(&self) -> Option<&filelisting::Item> {
        self.listing.selected_item()
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

    // TODO: select those before it
    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        if path.as_ref() == self.root_path {
            return Ok(());
        }
        let open_index = self
            .listing
            .iter()
            .position(|(_, item)| item.path() == path.as_ref())
            .with_context(|| format!("{} not found in listing", path.as_ref().display()))?;
        self.listing.select(open_index);

        Ok(())
    }

    pub fn open_all(&mut self) {
        self.listing.unfold_all();
    }

    pub fn close_all(&mut self) {
        self.listing.fold_all();
    }

    pub fn open_under(&mut self, _location: &mut Vec<usize>) {
        todo!()
    }

    pub fn close_under(&mut self, _location: &mut Vec<usize>) {
        todo!()
    }
}

impl Drawable for Filetree {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let mut state = ListState::default();
        state.select(self.listing.selected());
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

                    const CLOSED_SYMBOL: &str = "\u{25b6} ";
                    const OPENED_SYMBOL: &str = "\u{25bc} ";
                    let icon = if !item.is_file() {
                        if self
                            .listing
                            .is_folded(item.path())
                            .expect("item should be in folded")
                        {
                            CLOSED_SYMBOL
                        } else {
                            OPENED_SYMBOL
                        }
                    } else {
                        ""
                    };
                    ListItem::new(format!(
                        "{}{icon}{file_name}",
                        " ".repeat(indent_amount * INDENT)
                    ))
                })
                .collect_vec(),
        )
        .highlight_style(Style::default().bg(Color::LightGreen).fg(Color::Black))
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
                let not_empty = !self.listing.is_empty();
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
                        if let Some(item) = self.listing.selected_item() {
                            self.queue.add(AppEvent::OpenPopup(PendingOperation::DeleteFile(item.path().to_path_buf())));
                        }
                    },
                    self.config.filetree.diff_mode => self.queue.add(AppEvent::TogglePreviewMode),
                    self.config.filetree.git_filter => {
                        self.status_cache.as_ref().map_or_else(|| {
                            warn!("no git status to filter for");
                        }, |_cache| {
                            info!("filtered for modified files");
                            todo!("filter for files here");
                        });
                    },
                    self.config.filetree.search => self
                        .queue
                        .add(AppEvent::OpenInput(InputOperation::SearchFiles)),
                    self.config.filetree.clear => {
                        info!("refreshed filetree");
                        self.refresh().context("problem refreshing filetree")?;
                    },
                    self.config.open => match self.get_selected() {
                        Some(filelisting::Item::Dir(_)) => self.listing.toggle_fold(),
                        Some(filelisting::Item::File(file)) => self
                            .queue
                            .add(AppEvent::OpenFile(file.clone())),
                        None => {}
                    },
                    self.config.filetree.new_file => {
                        if let Some(selected) = self.listing.selected() {
                            let is_folded = self.listing.is_folded(selected).unwrap();
                            let add_path = match self.listing.selected_item().expect("should exist, checked at top of block") {
                                filelisting::Item::Dir(dir) if !is_folded => dir,
                                item => item.path().parent().expect("item should have parent"),
                            };
                            self.queue
                                .add(AppEvent::OpenInput(InputOperation::NewFile { at: add_path.to_path_buf() }));
                        }
                    },
                    self.config.filetree.new_dir => {
                        if let Some(selected) = self.listing.selected() {
                            let is_folded = self.listing.is_folded(selected).unwrap();
                            let add_path = match self.listing.selected_item().expect("should exist, checked at top of block") {
                                filelisting::Item::Dir(dir) if !is_folded => dir,
                                item => item.path().parent().expect("item should have parent"),
                            };
                            self.queue
                                .add(AppEvent::OpenInput(InputOperation::NewDir { at: add_path.to_path_buf() }));
                        }
                    },
                    self.config.filetree.close_all => self.close_all(),
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

// fn build_filetree<'a>(
//     tree: &'a Dir,
//     statuses: Option<&HashMap<PathBuf, Status>>,
//     config: Rc<Config>,
//     marks: &[PathBuf],
//     highlight: &[PathBuf],
// ) -> Vec<TreeItem<'a>> {
//     let mut items = Vec::new();
//     for item in tree {
//         let style = 'style: {
//             if highlight.iter().any(|path| path == item.path()) {
//                 break 'style config.filetree.searched_style.into();
//             }
//             if marks.iter().any(|path| path == item.path()) {
//                 break 'style config.filetree.marks_style.into();
//             }
//             statuses.map_or(Style::default(), |statuses| {
//                 statuses
//                     .get(item.path())
//                     .map_or(Style::default(), |status| match *status {
//                         Status::WT_NEW => Style::from(config.filetree.git_new_style),
//                         Status::WT_MODIFIED => Style::from(config.filetree.git_modified_style),
//                         Status::INDEX_MODIFIED | Status::INDEX_NEW => {
//                             Style::from(config.filetree.git_modified_style)
//                         }
//                         _ => Style::default(),
//                     })
//             })
//         };
//         let tree_item = match item {
//             Item::Dir(dir) => TreeItem::new(
//                 last_of_path(dir.path()),
//                 build_filetree(dir, statuses, Rc::clone(&config), marks, highlight),
//             )
//             .style(style),
//             Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())).style(style),
//         };
//         items.push(tree_item);
//     }
//     items
// }

/// Builds an `Override` that ignores certain paths
fn build_override_ignorer(root: impl AsRef<Path>, ignore: &[String]) -> Result<Override> {
    let mut override_builder = OverrideBuilder::new(root.as_ref());

    for pat in ignore.iter().map(|x| x.as_str()).chain(iter::once("/.git")) {
        override_builder
            .add(&format!("!{pat}"))
            .with_context(|| format!("failed to add glob for: \"!{pat}\""))?
            .add(&format!("!{pat}/**"))
            .with_context(|| format!("failed to add glob for: \"!{pat}/**\""))?;
    }
    override_builder
        .build()
        .context("failed to build override ignorer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dir::temp_files;
    use crate::{app::components::testing::*, config::FiletreeConfig};
    use test_log::test;

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
        assert_eq!(
            path.join("test"),
            filetree.listing.selected_item().unwrap().path()
        );

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
            .is_folded(filetree.listing.selected().unwrap())
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
        assert_eq!(3, filetree.listing.selected().unwrap());
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
        assert_eq!(0, filetree.listing.selected().unwrap());
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
    fn partial_refresh_delete_goes_to_same_item() {
        let temp = temp_files!("test/test.txt", "test/test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.listing.unfold_all();
        filetree.listing.select(1);
        filetree
            .partial_refresh(&RefreshData::Delete(
                filetree
                    .listing
                    .selected_item()
                    .unwrap()
                    .path()
                    .to_path_buf(),
            ))
            .unwrap();
        assert_eq!(1, filetree.listing.selected().unwrap());
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
                filetree
                    .listing
                    .selected_item()
                    .unwrap()
                    .path()
                    .to_path_buf(),
            ))
            .unwrap();
        assert_eq!(0, filetree.listing.selected().unwrap());
    }

    #[test]
    fn can_open_all() {
        let temp = temp_files!("test.txt", "test/test2.txt", "test2/test4/test.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());
        filetree.open_all();
        assert_eq!(6, filetree.listing.len());
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

    #[test]
    fn can_ignore() {
        let temp = temp_files!("test.txt", "test2.txt");
        let config = Config {
            filetree: FiletreeConfig {
                ignore: vec!["test2.txt".to_owned()],
                ..Default::default()
            },
            ..Default::default()
        };
        let filetree = Filetree::from_dir_with_config(
            temp.path(),
            Queue::new(),
            Rc::new(config),
            Rc::new(RefCell::new(Vec::new())),
        )
        .unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(1, filetree.listing.items().len());
        assert_eq!(
            Path::new("test.txt"),
            filetree.listing.items()[0].path().file_name().unwrap()
        );
    }

    #[test]
    fn can_full_refresh_to_account_for_new_files() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        std::fs::File::create(temp.path().join("new.txt")).unwrap();

        assert_eq!(2, filetree.listing.len());
        assert!(filetree.refresh().is_ok());
        assert_eq!(
            Path::new("new.txt"),
            filetree
                .listing
                .items()
                .get(2)
                .unwrap()
                .path()
                .file_name()
                .unwrap()
        );
    }

    #[test]
    fn can_partial_refresh_to_add_files() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();
        std::fs::File::create(temp.path().join("new.txt")).unwrap();

        assert_eq!(2, filetree.listing.len());
        assert!(filetree
            .partial_refresh(&RefreshData::Add(temp.join("new.txt")))
            .is_ok());
        assert_eq!(
            Path::new("new.txt"),
            filetree
                .listing
                .items()
                .get(1)
                .unwrap()
                .path()
                .file_name()
                .unwrap()
        );
    }
}
