use crate::app::component::{Component, Drawable};
use crate::{
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use std::path::{PathBuf, MAIN_SEPARATOR};
use tui::{
    backend::Backend,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear},
    Frame,
};
use tui_textarea::{CursorMove, Input, Key, TextArea};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputOperation {
    NewFile {
        at: PathBuf,
    },
    NewDir {
        at: PathBuf,
    },
    Command {
        to: PathBuf,
    },
    SearchFiles,
    #[default]
    NoOperations,
}

#[derive(Default)]
pub struct InputBox {
    pub operation: InputOperation,
    queue: Queue,
    text: String,
    /// Offset from back of `text`
    cursor_offset: u32,
}

impl InputBox {
    pub fn new(queue: Queue) -> Self {
        Self {
            text: String::new(),
            queue,
            operation: Default::default(),
            cursor_offset: 0,
        }
    }

    fn has_work(&self) -> bool {
        self.operation != InputOperation::NoOperations
    }

    fn reset(&mut self) {
        self.text = String::new();
        self.operation = InputOperation::NoOperations;
    }

    fn cursor_left(&mut self) {
        self.cursor_offset += 1;
        let len = self.text.len() as u32;
        if self.cursor_offset > len {
            self.cursor_offset = len;
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_offset == 0 {
            return;
        }
        self.cursor_offset -= 1;
    }

    fn cursor_pos(&self) -> usize {
        self.text.len() - self.cursor_offset as usize
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
            _ => Some(true),
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
                    key: Key::Right, ..
                } => self.cursor_right(),
                Input { key: Key::Left, .. } => self.cursor_left(),
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
                        InputOperation::Command { to } => {
                            // Perform string substitution for path
                            let cmd = self.text.replace("{}", &to.display().to_string());
                            self.queue.add(AppEvent::RunCommand(cmd))
                        }
                        InputOperation::SearchFiles => {
                            self.queue.add(AppEvent::SearchFiles(self.text.to_owned()))
                        }

                        InputOperation::NoOperations => unreachable!("checked in match guard"),
                    };
                    self.reset();
                }
                Input {
                    key: Key::Char('u'),
                    ctrl: true,
                    ..
                } => drop(self.text.drain(..self.cursor_pos())),
                Input {
                    key: Key::Delete | Key::Backspace,
                    ..
                } if self.text.len() as u32 > self.cursor_offset => {
                    self.text
                        .remove((self.text.len() - self.cursor_offset as usize) - 1);
                }
                Input {
                    key: Key::Char(k), ..
                } => self.text.insert(self.cursor_pos(), k),

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
        let area = ui::centered_rect_absolute(50, 3, area);
        let title = match self.operation {
            InputOperation::Command { .. } => "Run Command",
            InputOperation::NewDir { .. } => "New Directory",
            InputOperation::NewFile { .. } => "New File",
            InputOperation::SearchFiles => "Search",
            InputOperation::NoOperations => unreachable!("checked at top of method"),
        };
        let mut textarea = TextArea::default();
        textarea.insert_str(&self.text);
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(Alignment::Center)
                .border_style(if self.has_valid_input().expect("should have operation") {
                    Style::default().fg(Color::LightGreen)
                } else {
                    Style::default().fg(Color::Red)
                }),
        );
        for _ in 0..self.cursor_offset {
            textarea.move_cursor(CursorMove::Back);
        }
        f.render_widget(Clear, area);
        f.render_widget(textarea.widget(), area);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{super::testing::*, *};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use test_log::test;

    #[test]
    fn giving_operation_gives_work() {
        let mut input_box = InputBox::default();
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
            let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
        input_box.text = "text".to_owned();
        input_box.operation = InputOperation::NewFile { at: "/".into() };
        input_box.handle_event(&event).expect("should not error");
        assert_eq!(String::new(), input_box.text);
        assert_eq!(InputOperation::NoOperations, input_box.operation);
    }

    #[test]
    fn takes_no_input_with_no_work() {
        let events = input_events!(KeyCode::Char('h'), KeyCode::Char('i'));
        let mut input_box = InputBox::default();
        for event in events {
            input_box.handle_event(&event).expect("input should work");
        }
        assert_eq!(String::new(), input_box.text);
    }

    #[test]
    fn takes_input() {
        let events = input_events!(KeyCode::Char('h'), KeyCode::Char('i'));
        let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
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
        let mut input_box = InputBox::default();
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
            let mut input_box = InputBox::default();
            input_box.operation = operation;
            assert!(!input_box.has_valid_input().expect("should have work"))
        }
    }

    #[test]
    fn moving_cursor_does_not_go_past_text_on_right() {
        let mut input_box = InputBox::default();
        input_box.text = "testing".to_owned();
        input_box.cursor_right();
        assert_eq!(0, input_box.cursor_offset);
    }

    #[test]
    fn moving_cursor_does_not_go_past_text_on_left() {
        let mut input_box = InputBox::default();
        input_box.text = "test".to_owned();
        for _ in 0..5 {
            input_box.cursor_left();
        }
        assert_eq!(4, input_box.cursor_offset);
    }

    #[test]
    fn can_send_execute_command_to_queue() {
        let enter = input_event!(KeyCode::Enter);

        let mut input_box = InputBox::default();
        input_box.text = "testing {}".to_owned();
        input_box.operation = InputOperation::Command { to: "/".into() };
        input_box.handle_event(&enter).unwrap();

        assert_eq!(
            AppEvent::RunCommand("testing /".to_owned()),
            input_box.queue.pop().unwrap()
        );
    }

    #[test]
    fn deletes_where_cursor_is() {
        let mut input_box = InputBox::default();
        input_box.text = "testing".to_owned();
        input_box.operation = InputOperation::Command { to: "/".into() };

        let events = input_events!(KeyCode::Left, KeyCode::Delete);
        input_box.handle_event(&events[0]).unwrap();
        input_box.handle_event(&events[1]).unwrap();
        assert_eq!("testig".to_owned(), input_box.text);
    }

    #[test]
    fn inserts_where_cursor_is() {
        let mut input_box = InputBox::default();
        input_box.text = "testing".to_owned();
        input_box.operation = InputOperation::Command { to: "/".into() };

        let events = input_events!(KeyCode::Left, KeyCode::Char('n'));
        input_box.handle_event(&events[0]).unwrap();
        input_box.handle_event(&events[1]).unwrap();
        assert_eq!("testinng".to_owned(), input_box.text);
    }

    #[test]
    fn deletes_line_where_cursor_is() {
        let mut input_box = InputBox::default();
        input_box.text = "testing".to_owned();
        input_box.operation = InputOperation::Command { to: "/".into() };

        let events = input_events!(KeyCode::Left, KeyCode::Char('u'); KeyModifiers::CONTROL);
        input_box.handle_event(&events[0]).unwrap();
        input_box.handle_event(&events[1]).unwrap();
        assert_eq!("g".to_owned(), input_box.text);
    }
}
