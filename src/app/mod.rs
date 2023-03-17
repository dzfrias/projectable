pub mod component;
mod filetree;
mod input_box;
mod pending_popup;

use self::component::{Component, Drawable};
use crate::{
    event::ExternalEvent,
    queue::{AppEvent, Queue},
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use filetree::Filetree;
pub use input_box::*;
pub use pending_popup::*;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

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
}

impl App {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let queue = Queue::new();
        let app = App {
            path: path.as_ref().to_path_buf(),
            tree: Filetree::from_dir(&path, queue.clone())?,
            should_quit: false,
            pending: PendingPopup::new(queue.clone()),
            input_box: InputBox::new(queue.clone()),
            queue,
        };

        Ok(app)
    }

    pub fn update(&mut self) -> Result<Option<TerminalEvent>> {
        let app_event = if let Some(ev) = self.queue.pop() {
            ev
        } else {
            return Ok(None);
        };

        match app_event {
            AppEvent::OpenPopup(operation) => self.pending.operation = operation,
            AppEvent::DeleteFile(path) => {
                if path.is_file() {
                    fs::remove_file(path)?;
                } else {
                    fs::remove_dir_all(path)?;
                }
                self.tree.refresh()?;
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
        }
        Ok(None)
    }

    pub fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        let popup_open = self.pending.visible() || self.input_box.visible();
        self.tree.focus(!popup_open);

        self.pending.handle_event(ev)?;
        self.input_box.handle_event(ev)?;
        self.tree.handle_event(ev)?;

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

        let text = vec![
            Span::raw("hi").into(),
            Span::styled("Second line", Style::default().fg(Color::Red)).into(),
        ];
        let p = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });

        let block = Block::default().title("Block").borders(Borders::ALL);

        self.tree.draw(f, left_hand_layout[0])?;
        f.render_widget(block, left_hand_layout[1]);
        f.render_widget(p, main_layout[1]);
        self.pending.draw(f, area)?;
        self.input_box.draw(f, area)?;

        Ok(())
    }
}
