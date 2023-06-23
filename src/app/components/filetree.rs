use crate::{
    app::{component::*, InputOperation, PendingOperation},
    config::Config,
    external_event::{ExternalEvent, RefreshData},
    filelisting::{FileListing, Item},
    marks::Marks,
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
    cell::{Cell, RefCell},
    collections::HashMap,
    iter,
    path::{Path, PathBuf},
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HiddenVisibility {
    Visible,
    Hidden,
}

pub struct Filetree {
    is_focused: bool,
    listing: FileListing,
    root_path: PathBuf,
    queue: Queue,
    repo: Option<Repository>,
    status_cache: Option<HashMap<PathBuf, Status>>,
    config: Rc<Config>,
    state: Cell<ListState>,
    marks: Rc<RefCell<Marks>>,
    is_showing_hidden: bool,
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
            state: ListState::default().into(),
            is_showing_hidden: false,
        };
        tree.populate_status_cache();
        if let Some(item) = tree.get_selected() {
            queue.add(AppEvent::PreviewFile(item.path().to_owned()));
        }
        tree.listing.fold_all();
        tree.sync_selected();
        Ok(tree)
    }

    pub fn from_dir_with_config(
        path: impl AsRef<Path>,
        queue: Queue,
        config: Rc<Config>,
        marks: Rc<RefCell<Marks>>,
    ) -> Result<Self> {
        let overrides = build_override_ignorer(&path, &config.filetree.ignore)?;
        let mut listing = FileListing::new(
            &WalkBuilder::new(path.as_ref())
                .overrides(overrides)
                .hidden(!config.filetree.show_hidden_by_default)
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
        let mut listing = FileListing::new(
            &self
                .build_walkbuilder(HiddenVisibility::Hidden)?
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
                if self.get_selected().is_some_and(|item| item.path() == path) {
                    self.queue.add(AppEvent::PreviewFile(path.clone()));
                }
            }
            RefreshData::Add(path) => {
                if !path.exists() {
                    return Ok(());
                }
                if path.is_dir() {
                    self.listing.add(Item::Dir(path.clone()));
                } else {
                    self.listing.add(Item::File(path.clone()));
                }
                self.populate_status_cache();
                if self.get_selected().is_some_and(|item| item.path() == path) {
                    self.queue.add(AppEvent::PreviewFile(path.clone()));
                }
            }
        }

        Ok(())
    }

    pub fn move_item(&mut self, old: impl AsRef<Path>, new: impl AsRef<Path>) -> Result<()> {
        self.listing
            .mv(old.as_ref(), new)
            .context("error moving item")?;
        self.populate_status_cache();
        Ok(())
    }

    pub fn get_selected(&self) -> Option<&Item> {
        self.listing.selected_item()
    }

    pub fn open_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        if path.as_ref() == self.root_path {
            return Ok(());
        }

        self.listing.select(path.as_ref());
        self.queue
            .add(AppEvent::PreviewFile(path.as_ref().to_path_buf()));
        self.sync_selected();

        Ok(())
    }

    pub fn open_all(&mut self) {
        self.listing.unfold_all();
    }

    pub fn close_all(&mut self) {
        self.listing.fold_all();
    }

    pub fn open_under(&mut self) {
        self.listing
            .selected()
            .map(|selected| self.listing.unfold_under(selected));
    }

    pub fn close_under(&mut self) {
        self.listing
            .selected()
            .map(|selected| self.listing.fold_under(selected));
    }

    pub fn filter_include(&mut self, items: &[PathBuf]) -> Result<()> {
        let items = self
            .build_walkbuilder(HiddenVisibility::Hidden)?
            .filter(|entry_path| {
                items
                    .iter()
                    .any(|path| path == entry_path || entry_path.starts_with(path))
            })
            .collect_vec();

        self.listing = FileListing::new(&items);

        Ok(())
    }

    pub fn toggle_dotfiles(&mut self) -> Result<()> {
        let items = self
            .build_walkbuilder(if self.is_showing_hidden {
                HiddenVisibility::Hidden
            } else {
                HiddenVisibility::Visible
            })?
            .collect_vec();
        self.is_showing_hidden = !self.is_showing_hidden;

        self.listing = FileListing::new(&items);
        self.listing.fold_all();

        info!("toggling visibility of dotfiles");

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

    fn sync_selected(&mut self) {
        self.state.get_mut().select(self.listing.selected());
    }

    fn build_walkbuilder(
        &self,
        show_dotfiles: HiddenVisibility,
    ) -> Result<impl Iterator<Item = PathBuf> + '_> {
        let overrides = build_override_ignorer(&self.root_path, &self.config.filetree.ignore)?;
        Ok(WalkBuilder::new(&self.root_path)
            .overrides(overrides)
            .hidden(show_dotfiles != HiddenVisibility::Visible)
            .build()
            .filter_map(|entry| entry.ok().map(|entry| entry.into_path()))
            .filter(|entry_path| entry_path != &self.root_path))
    }
}

impl Drawable for Filetree {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let mut state = self.state.take();
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

                    const CLOSED_SYMBOL: char = '\u{25b6}';
                    const OPENED_SYMBOL: char = '\u{25bc}';
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
                        self.status_cache.as_ref().map_or(' ', |cache| {
                            cache.get(item.path()).map_or(' ', |status| match *status {
                                Status::WT_NEW => '+',
                                Status::INDEX_MODIFIED
                                | Status::INDEX_NEW
                                | Status::WT_MODIFIED => '~',
                                _ => ' ',
                            })
                        })
                    };
                    let mut style = if !self
                        .marks
                        .borrow()
                        .marks
                        .iter()
                        .any(|path| path == item.path())
                    {
                        self.status_cache
                            .as_ref()
                            .map_or(Style::default(), |cache| {
                                cache.get(item.path()).map_or(Style::default(), |status| {
                                    match *status {
                                        Status::WT_NEW => {
                                            Style::from(self.config.filetree.git_new_style)
                                        }
                                        Status::INDEX_MODIFIED => {
                                            Style::from(self.config.filetree.git_added_style)
                                        }
                                        Status::WT_MODIFIED => {
                                            Style::from(self.config.filetree.git_modified_style)
                                        }
                                        _ => Style::default(),
                                    }
                                })
                            })
                    } else {
                        self.config.filetree.marks_style.into()
                    };
                    if style == Style::default() && !item.is_file() {
                        style = self.config.filetree.dir_style.into();
                    }
                    ListItem::new(format!(
                        "{}{icon} {file_name}",
                        " ".repeat(indent_amount * INDENT)
                    ))
                    .style(style)
                })
                .collect_vec(),
        )
        .highlight_style(self.config.selected.into())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.config.filetree.border_color.into()),
        );
        f.render_stateful_widget(list, area, &mut state);
        self.state.set(state);

        Ok(())
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
                        if let Some(cache) = self.status_cache.as_ref() {
                            info!("filtered for modified files");
                            self.filter_include(cache.keys().cloned().collect_vec().as_ref())?;
                        } else {
                            warn!("no git status to filter for");
                        }
                    },
                    self.config.filetree.search => self.queue.add(AppEvent::SearchFiles(self.listing.all_items().iter().map(|item| item.path().to_path_buf()).collect())),
                    self.config.filetree.clear => {
                        info!("refreshed filetree");
                        self.refresh().context("problem refreshing filetree")?;
                    },
                    self.config.open => match self.get_selected() {
                        Some(Item::Dir(_)) => self.listing.toggle_fold(),
                        Some(Item::File(file)) => self
                            .queue
                            .add(AppEvent::OpenFile(file.clone())),
                        None => {}
                    },
                    self.config.filetree.new_file => {
                        if let Some(selected) = self.listing.selected() {
                            let is_folded = self.listing.is_folded(selected).unwrap();
                            let add_path = match self.listing.selected_item().expect("should exist, checked at top of block") {
                                Item::Dir(dir) if !is_folded => dir,
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
                                Item::Dir(dir) if !is_folded => dir,
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
                    self.config.filetree.open_under => self.open_under(),
                    self.config.filetree.close_under => self.close_under(),
                    self.config.filetree.show_dotfiles => self.toggle_dotfiles()?,
                    self.config.filetree.rename => {
                        if let Some(selected) = self.get_selected() {
                            self.queue.add(AppEvent::OpenInput(InputOperation::Rename { to: selected.path().to_path_buf() }))
                        }
                    },
                    _ => {
                        let key: crate::config::Key = key.into();
                        if let Some(cmd) = self.config.commands.get(&key) {
                            if let Some(selected) = self.get_selected() {
                                let new_cmd =
                                    cmd.replace("{}", &selected.path().as_os_str().to_string_lossy());
                                if new_cmd.contains("{...}") {
                                    self.queue
                                        .add(AppEvent::OpenInput(InputOperation::SpecialCommand(new_cmd)));
                                } else {
                                    self.queue.add(AppEvent::RunCommand(new_cmd));
                                }
                            }
                        };

                        refresh_preview = false;
                    },
                }
                if !refresh_preview {
                    self.state.get_mut().select(self.listing.selected());
                    return Ok(());
                }
                if let Some(item) = self.get_selected() {
                    self.queue
                        .add(AppEvent::PreviewFile(item.path().to_owned()));
                }
            }
            _ => {}
        }

        self.sync_selected();

        Ok(())
    }
}

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
    use crate::{app::components::testing::*, config::FiletreeConfig};
    use collect_all::collect;
    use smallvec::smallvec;
    use test_log::test;

    /// Create temporary files and return the temp dir
    macro_rules! temp_files {
    ($($name:expr),*) => {
            {
                #[allow(unused_imports)]
                use ::assert_fs::prelude::*;

                let __temp = ::assert_fs::TempDir::new().unwrap();
                $(
                    __temp.child($name).touch().unwrap();
                 )*
                __temp
            }
        };
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
        assert!(filetree.queue.contains(&AppEvent::SearchFiles(Vec::new())));
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
        let event = ExternalEvent::PartialRefresh(smallvec![RefreshData::Delete(
            "does_not_exist.txt".into()
        )]);
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
            Rc::new(RefCell::new(Marks::default())),
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
        assert_eq!(3, filetree.listing.len());
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
                .get(0)
                .unwrap()
                .path()
                .file_name()
                .unwrap()
        );
    }

    #[test]
    fn can_filter_include_certain_files() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();

        assert!(filetree
            .filter_include(&[temp.path().join("test.txt")])
            .is_ok());
        assert_eq!(
            vec![&Item::File(temp.path().join("test.txt"))],
            filetree.listing.items()
        );
    }

    #[test]
    fn can_show_hidden_files() {
        let temp = temp_files!("test.txt", ".test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();

        assert_eq!(1, filetree.listing.len());
        assert!(filetree.toggle_dotfiles().is_ok());
        assert!(filetree
            .listing
            .items()
            .contains(&&Item::File(temp.join(".test2.txt"))));
        assert_eq!(2, filetree.listing.len());
    }

    #[test]
    fn toggles_hidden_files() {
        let temp = temp_files!("test.txt", ".test2.txt");
        let mut filetree = Filetree::from_dir(temp.path(), Queue::new()).unwrap();

        assert_eq!(1, filetree.listing.len());
        assert!(filetree.toggle_dotfiles().is_ok());
        assert_eq!(2, filetree.listing.len());
        assert!(filetree.toggle_dotfiles().is_ok());
        assert_eq!(
            vec![&Item::File(temp.join("test.txt"))],
            filetree.listing.items()
        );
    }

    #[test]
    fn can_show_hidden_on_initialization() {
        let temp = temp_files!("test.txt", ".test2.txt");
        let config = Config {
            filetree: FiletreeConfig {
                show_hidden_by_default: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let filetree = Filetree::from_dir_with_config(
            temp.path(),
            Queue::new(),
            Rc::new(config),
            Default::default(),
        )
        .unwrap();

        assert_eq!(2, filetree.listing.len());
    }

    #[test]
    fn arbitrary_keys_are_fed_into_custom_commands() {
        let temp = temp_files!("test.txt", "test2.txt");
        let config = Config {
            commands: collect![HashMap<_, _>: (crate::config::Key::normal('z'), "testing".to_owned())],
            ..Default::default()
        };
        let mut filetree = Filetree::from_dir_with_config(
            temp.path(),
            Queue::new(),
            Rc::new(config),
            Default::default(),
        )
        .unwrap();

        assert!(filetree
            .handle_event(&input_event!(KeyCode::Char('z')))
            .is_ok());
        assert!(filetree
            .queue
            .contains(&AppEvent::RunCommand("testing".to_owned())));
    }

    #[test]
    fn custom_commands_are_performed_with_substitutions() {
        let temp = temp_files!("test.txt");
        let config = Config {
            commands: collect![HashMap<_, _>:
                (crate::config::Key::normal('z'), "vim {}".to_owned()),
                (crate::config::Key::normal('x'), "nvim {...}".to_owned())],
            ..Default::default()
        };
        let mut filetree = Filetree::from_dir_with_config(
            temp.path(),
            Queue::new(),
            Rc::new(config),
            Default::default(),
        )
        .unwrap();

        assert!(filetree
            .handle_event(&input_event!(KeyCode::Char('z')))
            .is_ok());
        assert!(filetree.queue.contains(&AppEvent::RunCommand(format!(
            "vim {}",
            temp.join("test.txt").display()
        ))));
        assert!(filetree
            .handle_event(&input_event!(KeyCode::Char('x')))
            .is_ok());
        assert!(filetree
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::SpecialCommand(
                "nvim {...}".to_owned()
            ))));
    }
}
