use std::{cell::Cell, path::PathBuf};

use super::component::{Component, Drawable};
use crate::{
    event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, PartialEq, Eq, Default)]
pub enum PendingOperations {
    DeleteFile(PathBuf),
    #[default]
    NoPending,
}

pub struct PendingPopup {
    pub operation: PendingOperations,
    pub state: Cell<ListState>,
    queue: Queue,
}

impl PendingPopup {
    pub fn new(queue: Queue) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            queue,
            state: state.into(),
            operation: Default::default(),
        }
    }

    fn has_work(&self) -> bool {
        self.operation != PendingOperations::NoPending
    }

    fn reset_work(&mut self) {
        self.operation = PendingOperations::NoPending;
        self.state = ListState::default().into();
        self.state.get_mut().select(Some(0));
    }

    fn select_next(&mut self) {
        let current = self.selected();
        if current == 1 {
            return;
        }
        self.state.get_mut().select(Some(current + 1));
    }

    fn select_prev(&mut self) {
        let current = self.selected();
        if current == 0 {
            return;
        }
        self.state.get_mut().select(Some(current - 1));
    }

    fn selected(&mut self) -> usize {
        self.state.get_mut().selected().unwrap_or_else(|| {
            self.state.get_mut().select(Some(0));
            0
        })
    }
}

impl Component for PendingPopup {
    fn visible(&self) -> bool {
        self.has_work()
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        if let ExternalEvent::Crossterm(Event::Key(key)) = ev {
            match key {
                KeyEvent {
                    code: KeyCode::Char('j'),
                    ..
                } => self.select_next(),
                KeyEvent {
                    code: KeyCode::Char('k'),
                    ..
                } => self.select_prev(),
                KeyEvent {
                    code: KeyCode::Esc | KeyCode::Char('q'),
                    ..
                } => self.reset_work(),
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } => {
                    let selected = self
                        .state
                        .get_mut()
                        .selected()
                        .expect("should always have something selected");
                    if selected == 1 {
                        self.reset_work();
                        return Ok(());
                    }
                    let event = match &self.operation {
                        PendingOperations::DeleteFile(path) => {
                            AppEvent::DeleteFile(path.to_owned())
                        }
                        PendingOperations::NoPending => {
                            panic!("should not have no pending work during confirmation")
                        }
                    };
                    self.queue.add(event);
                    self.reset_work();
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drawable for PendingPopup {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        let items = [ListItem::new("Confirm"), ListItem::new("Deny")];
        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
        let area = ui::centered_rect(30, 20, area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Percentage(50)].as_ref())
            .horizontal_margin(2)
            .vertical_margin(2)
            .split(area);
        f.render_widget(Clear, area);
        f.render_widget(
            Block::default()
                .title("Confirm")
                .borders(Borders::ALL)
                .title_alignment(Alignment::Center),
            area,
        );
        f.render_widget(
            Paragraph::new("Are you sure you want to delete this file/directory?")
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            layout[0],
        );
        let mut state = self.state.take();
        f.render_stateful_widget(list, layout[1], &mut state);
        self.state.set(state);
        Ok(())
    }
}
