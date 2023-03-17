use super::component::{Component, Drawable};
use crate::{
    event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use std::path::{PathBuf, MAIN_SEPARATOR};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use tui_textarea::{Input, Key, TextArea};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputOperation {
    NewFile {
        at: PathBuf,
    },
    NewDir {
        at: PathBuf,
    },
    #[default]
    NoOperations,
}

pub struct InputBox {
    pub operation: InputOperation,
    queue: Queue,
    text: String,
}

impl InputBox {
    pub fn new(queue: Queue) -> Self {
        Self {
            text: String::new(),
            queue,
            operation: Default::default(),
        }
    }

    fn has_work(&self) -> bool {
        self.operation != InputOperation::NoOperations
    }

    fn reset(&mut self) {
        self.text = String::new();
        self.operation = InputOperation::NoOperations;
    }

    fn has_valid_input(&self) -> Option<bool> {
        if self.text.is_empty() {
            return Some(false);
        }
        match self.operation {
            InputOperation::NewFile { .. } | InputOperation::NewDir { .. } => {
                if MAIN_SEPARATOR == '\\' {
                    Some(!(self.text.contains('/') || self.text.contains('\\')))
                } else {
                    Some(!self.text.contains('/'))
                }
            }
            InputOperation::NoOperations => None,
        }
    }
}

impl Component for InputBox {
    fn visible(&self) -> bool {
        self.has_work()
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        if let ExternalEvent::Crossterm(ev) = ev {
            let input_event: Input = ev.clone().into();
            match input_event {
                Input { key: Key::Esc, .. } => self.reset(),
                Input {
                    key: Key::Enter, ..
                } if self
                    .has_valid_input()
                    .expect("should not be called with no work") =>
                {
                    match &self.operation {
                        InputOperation::NewFile { at } => {
                            self.queue
                                .add(AppEvent::NewFile(at.join(self.text.as_str())));
                        }
                        InputOperation::NewDir { at } => self
                            .queue
                            .add(AppEvent::NewDir(at.join(self.text.as_str()))),
                        InputOperation::NoOperations => unreachable!("checked in match guard"),
                    };
                    self.reset();
                }
                Input {
                    key: Key::Char('u'),
                    ctrl: true,
                    ..
                } => self.text.clear(),
                Input {
                    key: Key::Delete | Key::Backspace,
                    ..
                } => drop(self.text.pop()),
                Input {
                    key: Key::Char(k), ..
                } => {
                    self.text.push(k);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drawable for InputBox {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }
        let area = ui::centered_rect(25, 20, area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Percentage(30),
                    Constraint::Min(1),
                    Constraint::Length(3),
                ]
                .as_ref(),
            )
            .horizontal_margin(2)
            .vertical_margin(1)
            .split(area);
        let mut textarea = TextArea::default();
        textarea.insert_str(&self.text);
        textarea.set_block(Block::default().borders(Borders::ALL).border_style(
            if self.has_valid_input().expect("should have operation") {
                Style::default()
            } else {
                Style::default().fg(Color::Red)
            },
        ));
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Create File")
            .title_alignment(Alignment::Center);
        let p =
            Paragraph::new("What would you like to name the file?").alignment(Alignment::Center);
        f.render_widget(Clear, area);
        f.render_widget(block, area);
        f.render_widget(p, layout[1]);
        f.render_widget(textarea.widget(), layout[3]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use super::{super::testing::*, *};

    #[test]
    fn giving_operation_gives_work() {
        let mut input_box = InputBox::new(Queue::new());
        assert!(!input_box.has_work());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        assert!(input_box.has_work());
    }

    #[test]
    fn cannot_add_slash_when_creating_file_or_dir() {
        for operation in [
            InputOperation::NewDir { at: "/".into() },
            InputOperation::NewFile { at: "/".into() },
        ] {
            let mut input_box = InputBox::new(Queue::new());
            input_box.operation = operation;
            input_box.text = "should not work /".to_owned();
            assert!(!input_box.has_valid_input().expect("should have work"));
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn invalid_input_with_backslash_when_creating_file_or_dir_on_windows() {
        for operation in [
            InputOperation::NewDir { at: "/".into() },
            InputOperation::NewFile { at: "/".into() },
        ] {
            let mut input_box = InputBox::new(Queue::new());
            input_box.operation = operation;
            input_box.text = "should not work \\".to_owned();
            assert!(!input_box.has_valid_input().expect("should have work"));
        }
    }

    #[test]
    fn reset_on_esc() {
        let event = input_event!(KeyCode::Esc);
        let mut input_box = InputBox::new(Queue::new());
        input_box.text = "text".to_owned();
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        input_box.handle_event(&event).expect("should not error");
        assert_eq!(String::new(), input_box.text);
        assert_eq!(InputOperation::NoOperations, input_box.operation);
    }

    #[test]
    fn takes_no_input_with_no_work() {
        let events = input_events!(KeyCode::Char('h'), KeyCode::Char('i'));
        let mut input_box = InputBox::new(Queue::new());
        for event in events {
            input_box.handle_event(&event).expect("input should work");
        }
        assert_eq!(String::new(), input_box.text);
    }

    #[test]
    fn takes_input() {
        let events = input_events!(KeyCode::Char('h'), KeyCode::Char('i'));
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        for event in events {
            input_box.handle_event(&event).expect("input should work");
        }
        assert_eq!("hi".to_owned(), input_box.text);
    }

    #[test]
    fn can_delete() {
        let events = input_events!(
            KeyCode::Char('h'),
            KeyCode::Char('i'),
            KeyCode::Backspace,
            KeyCode::Delete
        );
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        for event in events {
            input_box.handle_event(&event).expect("input should work");
        }
        assert_eq!(String::new(), input_box.text);
    }

    #[test]
    fn can_delete_whole_line() {
        let events = input_events!(KeyCode::Char('h'), KeyCode::Char('i'));
        let delete_all = ExternalEvent::Crossterm(Event::Key(KeyEvent {
            code: KeyCode::Char('u'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }));
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        for event in events {
            input_box.handle_event(&event).expect("input should work");
        }
        input_box
            .handle_event(&delete_all)
            .expect("input should work");
        assert_eq!(String::new(), input_box.text);
    }

    #[test]
    fn can_send_new_dir_event() {
        let event = input_event!(KeyCode::Enter);
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewDir { at: "/".into() };
        input_box.text = "hello_world".to_owned();
        input_box.handle_event(&event).expect("input should work");
        assert_eq!(
            AppEvent::NewDir("/hello_world".into()),
            input_box.queue.pop().expect("should have sent event")
        );
    }

    #[test]
    fn can_send_new_file_event() {
        let event = input_event!(KeyCode::Enter);
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        input_box.text = "hello_world.txt".to_owned();
        input_box.handle_event(&event).expect("input should work");
        assert_eq!(
            AppEvent::NewFile("/hello_world.txt".into()),
            input_box.queue.pop().expect("should have sent event")
        );
    }

    #[test]
    fn resets_after_option_entered() {
        let event = input_event!(KeyCode::Enter);
        let mut input_box = InputBox::new(Queue::new());
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        input_box.text = "test".to_owned();
        input_box.handle_event(&event).expect("input should work");
        assert!(input_box.text.is_empty());
        assert_eq!(InputOperation::NoOperations, input_box.operation);
    }

    #[test]
    fn cannot_take_empty_input() {
        for operation in [
            InputOperation::NewFile { at: "/".into() },
            InputOperation::NewDir { at: "/".into() },
        ] {
            let mut input_box = InputBox::new(Queue::new());
            input_box.operation = operation;
            assert!(!input_box.has_valid_input().expect("should have work"))
        }
    }
}
