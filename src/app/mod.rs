pub mod component;
mod components;

use self::component::{Component, Drawable};
pub use self::components::*;
use crate::{
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use rust_search::SearchBuilder;
use std::{
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders},
    Frame,
};

/// Event that is sent back up to main.rs
#[derive(Debug)]
pub enum TerminalEvent {
    OpenFile(PathBuf),
}

pub struct App {
    tree: Filetree,
    path: PathBuf,
    should_quit: bool,
    queue: Queue,
    pending: PendingPopup,
    input_box: InputBox,
    previewer: PreviewFile,
}

impl App {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let queue = Queue::new();
        Ok(App {
            path: path.as_ref().to_path_buf(),
            tree: Filetree::from_dir(&path, queue.clone())?,
            should_quit: false,
            pending: PendingPopup::new(queue.clone()),
            input_box: InputBox::new(queue.clone()),
            previewer: PreviewFile::default(),
            queue,
        })
    }

    /// Returns None if no events should be sent to the terminal
    pub fn update(&mut self) -> Result<Option<TerminalEvent>> {
        let app_event = if let Some(ev) = self.queue.pop() {
            ev
        } else {
            return Ok(None);
        };

        // Handle events from queue
        match app_event {
            AppEvent::OpenPopup(operation) => self.pending.operation = operation,
            AppEvent::DeleteFile(path) => {
                if path.is_file() {
                    fs::remove_file(path)?;
                } else {
                    fs::remove_dir_all(path)?;
                }
                self.tree.refresh()?;
                self.queue.add(AppEvent::PreviewFile(
                    self.tree.get_selected().unwrap().path().to_owned(),
                ));
            }
            AppEvent::OpenFile(path) => return Ok(Some(TerminalEvent::OpenFile(path))),
            AppEvent::OpenInput(op) => self.input_box.operation = op,
            AppEvent::NewFile(path) => {
                File::create(path)?;
                self.tree.refresh()?;
            }
            AppEvent::NewDir(path) => {
                fs::create_dir(path)?;
                self.tree.refresh()?;
            }
            AppEvent::PreviewFile(path) => self.previewer.preview_file(path)?,
            AppEvent::TogglePreviewMode => self.previewer.toggle_mode(),
            AppEvent::RunCommand(cmd) => {
                if cfg!(target_os = "windows") {
                    let output = Command::new("cmd").arg("/C").arg(&cmd).output()?;
                    // TODO: Make output actually do something
                    dbg!(String::from_utf8_lossy(&output.stdout).to_string());
                } else {
                    let output = Command::new(env::var("SHELL").unwrap_or("sh".to_owned()))
                        .arg("-c")
                        .arg(&cmd)
                        .output()?;
                    // TODO: Make output actually do something
                    dbg!(String::from_utf8_lossy(&output.stdout).to_string());
                }
            }
            AppEvent::SearchFiles(search) => {
                let results = SearchBuilder::default()
                    .location(".")
                    .search_input(search)
                    .ignore_case()
                    .hidden()
                    .build()
                    .collect::<Vec<_>>();
                self.tree.only_include(
                    results
                        .into_iter()
                        .map(|path| path.into())
                        .collect::<Vec<_>>(),
                )?;
            }
        }
        Ok(None)
    }

    pub fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        let popup_open = self.pending.visible() || self.input_box.visible();
        // Do not give the Filetree focus if there are any popups open
        self.tree.focus(!popup_open);
        self.previewer.focus(!popup_open);

        self.pending.handle_event(ev)?;
        self.input_box.handle_event(ev)?;
        self.tree.handle_event(ev)?;
        self.previewer.handle_event(ev)?;

        if popup_open {
            return Ok(());
        }
        match ev {
            ExternalEvent::Crossterm(Event::Key(KeyEvent { code, .. })) => match code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drawable for App {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .horizontal_margin(1)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);
        let left_hand_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(main_layout[0]);

        let block = Block::default().title("Block").borders(Borders::ALL);

        self.tree.draw(f, left_hand_layout[0])?;
        f.render_widget(block, left_hand_layout[1]);
        self.previewer.draw(f, main_layout[1])?;
        self.pending.draw(f, area)?;
        self.input_box.draw(f, area)?;

        Ok(())
    }
}
