use std::{cell::Cell, path::PathBuf};

use crate::app::component::{Component, Drawable};
use crate::{
    external_event::ExternalEvent,
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

#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub enum PendingOperation {
    DeleteFile(PathBuf),
    #[default]
    NoPending,
}

pub struct PendingPopup {
    pub operation: PendingOperation,
    pub state: Cell<ListState>,
    queue: Queue,
}

impl Default for PendingPopup {
    fn default() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            state: state.into(),
            queue: Queue::default(),
            operation: PendingOperation::default(),
        }
    }
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
        self.operation != PendingOperation::NoPending
    }

    fn reset_work(&mut self) {
        self.operation = PendingOperation::NoPending;
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

    fn selected(&self) -> usize {
        let state = self.state.take();
        let selected = state.selected().expect("should have selected something");
        self.state.set(state);
        selected
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
                    // Delete option
                    if selected == 1 {
                        self.reset_work();
                        return Ok(());
                    }
                    let event = match &self.operation {
                        PendingOperation::DeleteFile(path) => AppEvent::DeleteFile(path.to_owned()),
                        PendingOperation::NoPending => {
                            unreachable!("has work, checked at top of method")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;
    use test_log::test;

    #[test]
    fn new_popup_selects_first_item() {
        let popup = PendingPopup::default();
        assert_eq!(0, popup.selected())
    }

    #[test]
    fn selecting_next_does_not_go_over_num_of_items() {
        let mut popup = PendingPopup::default();
        popup.select_next();
        popup.select_next();
        popup.select_next();
        assert_eq!(1, popup.selected());
    }

    #[test]
    fn selecting_prev_does_not_go_below_num_of_items() {
        let mut popup = PendingPopup::default();
        popup.select_prev();
        popup.select_prev();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn receives_no_input_when_has_no_work() {
        let down = input_event!(KeyCode::Char('j'));
        let mut popup = PendingPopup::default();
        popup.handle_event(&down).expect("should handle input");
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn can_go_up_and_down() {
        let down = input_event!(KeyCode::Char('j'));
        let up = input_event!(KeyCode::Char('k'));
        let mut popup = PendingPopup::default();
        popup.operation = PendingOperation::DeleteFile("/".into());
        popup.handle_event(&down).expect("should handle input");
        assert_eq!(1, popup.selected());
        popup.handle_event(&up).expect("should handle input");
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn sends_message_on_confirm() {
        let enter = input_event!(KeyCode::Enter);
        let mut popup = PendingPopup::default();
        popup.operation = PendingOperation::DeleteFile("/".into());
        popup.handle_event(&enter).expect("should handle input");
        assert!(popup.queue.pop().is_some());
    }

    #[test]
    fn sends_no_message_on_deny() {
        let events = input_events!(KeyCode::Char('j'), KeyCode::Enter);
        let mut popup = PendingPopup::default();
        popup.operation = PendingOperation::DeleteFile("/".into());
        for event in events {
            popup.handle_event(&event).expect("should handle input");
        }
        assert!(popup.queue.pop().is_none())
    }
}
