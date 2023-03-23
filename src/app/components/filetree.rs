use crate::{
    app::{component::*, InputOperation, PendingOperation},
    config::Config,
    dir::*,
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    switch,
};
use anyhow::{anyhow, Result};
use crossterm::event::Event;
use git2::{Repository, Status};
use log::{info, warn};
use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

pub struct Filetree {
    state: Cell<TreeState>,
    is_focused: bool,
    dir: Dir,
    root_path: PathBuf,
    queue: Queue,
    only_included: bool,
    repo: Option<Repository>,
    status_cache: Option<HashMap<PathBuf, Status>>,
    config: Rc<Config>,
}

impl Filetree {
    pub fn from_dir(path: impl AsRef<Path>, queue: Queue) -> Result<Self> {
        let tree = DirBuilder::new(&path).dirs_first(true).build()?;
        let mut state = TreeState::default();
        state.select_first();
        let mut tree = Filetree {
            root_path: path.as_ref().to_path_buf(),
            state: state.into(),
            is_focused: true,
            queue: queue.clone(),
            dir: tree,
            only_included: false,
            repo: Repository::open(path.as_ref().join(".git")).ok(),
            status_cache: None,
            config: Rc::new(Config::default()),
        };
        tree.populate_status_cache();
        if let Some(item) = tree.get_selected() {
            queue.add(AppEvent::PreviewFile(item.path().to_owned()));
        }
        Ok(tree)
    }

    pub fn from_dir_with_config(
        path: impl AsRef<Path>,
        queue: Queue,
        config: Rc<Config>,
    ) -> Result<Self> {
        Ok(Filetree {
            repo: if config.filetree.use_git {
                Repository::open(path.as_ref().join(".git")).ok()
            } else {
                None
            },
            config: Rc::clone(&config),
            ..Self::from_dir(path, queue)?
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path)
            .dirs_first(true)
            .ignore(self.config.filetree.ignore.clone())
            .build()?;
        self.dir = tree;
        self.only_included = false;
        self.populate_status_cache();

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        Ok(())
    }

    pub fn get_selected(&self) -> Option<&Item> {
        let state = self.state.take();
        let item = self.dir.nested_child(&state.selected())?;
        self.state.set(state);
        Some(item)
    }

    pub fn only_include(&mut self, include: Vec<PathBuf>) -> Result<()> {
        self.dir = DirBuilder::new(&self.root_path)
            .dirs_first(true)
            .only_include(include)
            .build()?;
        self.only_included = true;

        if self.get_selected().is_none() {
            self.state.get_mut().select_first();
        }
        if let Some(selected) = self.get_selected() {
            self.queue
                .add(AppEvent::PreviewFile(selected.path().to_owned()));
        }
        Ok(())
    }

    pub fn populate_status_cache(&mut self) {
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
            .location_by_path(path)
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
        Ok(())
    }

    fn current_is_open(&mut self) -> bool {
        let selected = self.state.get_mut().selected();
        // Will return true if it was already closed
        let closed = self.state.get_mut().open(selected.clone());
        if closed {
            // Reverse the opening
            self.state.get_mut().close(&selected);
        }
        !closed
    }
}

impl Drawable for Filetree {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let items = build_filetree(
            &self.dir,
            self.status_cache.as_ref(),
            Rc::clone(&self.config),
        );
        let mut state = self.state.take();

        if self.only_included {
            let layout = Layout::default()
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(1),
                ])
                .margin(1)
                .split(area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(self.config.filetree.border_color.into());
            let tree = Tree::new(items).highlight_style(self.config.filetree.selected.into());
            let p = Paragraph::new("Some results may be filtered out ('\\' to reset)")
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .style(self.config.filetree.filtered_out_message.into());

            f.render_widget(block, area);
            f.render_widget(p, layout[0]);
            f.render_stateful_widget(tree, layout[2], &mut state);
        } else {
            let tree = Tree::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(self.config.filetree.border_color.into()),
                )
                .highlight_style(self.config.filetree.selected.into());
            f.render_stateful_widget(tree, area, &mut state);
        }

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

        let items = build_filetree(&self.dir, None, Rc::clone(&self.config));

        const JUMP_DOWN_AMOUNT: u8 = 3;
        match ev {
            ExternalEvent::RefreshFiletree => self.refresh()?,
            ExternalEvent::Crossterm(Event::Key(key)) => {
                let mut refresh_preview = true;
                let not_empty = !items.is_empty();
                switch! { key;
                    self.config.filetree.all_up => self.state.get_mut().select_first(),
                    self.config.filetree.all_down => self.state.get_mut().select_last(&items),
                    self.config.filetree.down, not_empty => self.state.get_mut().key_down(&items),
                    self.config.filetree.up, not_empty => self.state.get_mut().key_up(&items),
                    self.config.filetree.down_three, not_empty => {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_down(&items);
                        }
                    },
                    self.config.filetree.up_three, not_empty => {
                        for _ in 0..JUMP_DOWN_AMOUNT {
                            self.state.get_mut().key_up(&items);
                        }
                    },
                    self.config.filetree.exec_cmd, not_empty => {
                        if let Some(item) = self.get_selected() {
                             self.queue.add(AppEvent::OpenInput(InputOperation::Command {
                                 to: item.path().to_path_buf(),
                             }));
                         }
                    },
                    self.config.filetree.delete => {
                        if let Some(item) = self.get_selected() {
                            self.queue
                                .add(AppEvent::OpenPopup(PendingOperation::DeleteFile(
                                    item.path().to_path_buf(),
                            )));
                        }
                    },
                    self.config.filetree.diff_mode => self.queue.add(AppEvent::TogglePreviewMode),
                    self.config.filetree.git_filter => {
                        if let Some(cache) = self.status_cache.as_ref() {
                            info!(" filtered for modified files");
                            self.only_include(cache.keys().cloned().collect())?;
                        } else {
                            warn!(" no git status to filter for");
                        }
                    },
                    self.config.filetree.search => self
                        .queue
                        .add(AppEvent::OpenInput(InputOperation::SearchFiles)),
                    self.config.filetree.clear => {
                        info!(" refreshed filetree");
                        self.refresh()?;
                    },
                    self.config.filetree.open => match self.get_selected() {
                        Some(Item::Dir(_)) => self.state.get_mut().toggle_selected(),
                        Some(Item::File(file)) => self
                            .queue
                            .add(AppEvent::OpenFile(file.path().to_path_buf())),
                        None => {}
                    },
                    self.config.filetree.new_file => {
                        let opened = self.current_is_open();
                        let add_path = match self.get_selected() {
                            Some(Item::Dir(dir)) if opened => dir.path(),
                            Some(item) => item.path().parent().expect("item should have parent"),
                            None => return Ok(()),
                        };
                        self.queue
                            .add(AppEvent::OpenInput(InputOperation::NewFile { at: add_path.to_path_buf() }));
                    },
                    self.config.filetree.new_dir => {
                        let opened = self.current_is_open();
                        let add_path = match self.get_selected() {
                            Some(Item::Dir(dir)) if opened => dir.path(),
                            Some(item) => item.path().parent().expect("item should have parent"),
                            None => return Ok(()),
                        };
                        self.queue
                            .add(AppEvent::OpenInput(InputOperation::NewDir { at: add_path.to_path_buf() }));
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
) -> Vec<TreeItem<'a>> {
    let mut items = Vec::new();
    for item in tree {
        let style = statuses
            .map(|statuses| {
                statuses
                    .get(item.path())
                    .map(|status| match *status {
                        Status::WT_NEW => Style::from(config.filetree.git_new_style),
                        Status::WT_MODIFIED => Style::from(config.filetree.git_modified_style),
                        Status::INDEX_MODIFIED | Status::INDEX_NEW => {
                            Style::from(config.filetree.git_modified_style)
                        }
                        _ => Style::default(),
                    })
                    .unwrap_or(Style::default())
            })
            .unwrap_or(Style::default());
        let tree_item = match item {
            Item::Dir(dir) => TreeItem::new(
                last_of_path(dir.path()),
                build_filetree(dir, statuses, Rc::clone(&config)),
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
        )
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
        assert_eq!(path.join("test"), filetree.get_selected().unwrap().path());

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
        filetree.state.get_mut().toggle_selected();
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
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        let enter = input_event!(KeyCode::Enter);
        filetree
            .handle_event(&enter)
            .expect("should be able to handle keypress");
        assert_eq!(vec![vec![0]], filetree.state.get_mut().get_all_opened());
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
        assert_eq!(3, filetree.state.get_mut().selected()[0])
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
        assert_eq!(0, filetree.state.get_mut().selected()[0])
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
            })))
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
            .contains(&AppEvent::OpenInput(InputOperation::SearchFiles)))
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
        assert_eq!(1, filetree.state.get_mut().get_all_opened().len())
    }
}
