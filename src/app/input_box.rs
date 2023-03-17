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

#[derive(Debug, PartialEq, Eq, Default)]
pub enum InputOperations {
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
    pub operation: InputOperations,
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
        self.operation != InputOperations::NoOperations
    }

    fn reset(&mut self) {
        self.text = String::new();
        self.operation = InputOperations::NoOperations;
    }

    fn has_valid_input(&self) -> Option<bool> {
        match self.operation {
            InputOperations::NewFile { .. } | InputOperations::NewDir { .. } => {
                if MAIN_SEPARATOR == '\\' {
                    Some(!(self.text.contains('/') || self.text.contains('\\')))
                } else {
                    Some(!self.text.contains('/'))
                }
            }
            InputOperations::NoOperations => None,
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
                        InputOperations::NewFile { at } => {
                            self.queue
                                .add(AppEvent::NewFile(at.join(self.text.as_str())));
                        }
                        InputOperations::NewDir { at } => self
                            .queue
                            .add(AppEvent::NewDir(at.join(self.text.as_str()))),
                        InputOperations::NoOperations => unreachable!("checked in match guard"),
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
