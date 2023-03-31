use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use globset::{Glob, GlobMatcher};
use itertools::Itertools;
use std::{cell::Cell, path::PathBuf, rc::Rc};

use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

use super::InputOperation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchState {
    Matched,
    NotMatched,
}

impl MatchState {
    pub fn is_matched(&self) -> bool {
        self == &MatchState::Matched
    }
}

#[derive(Debug, Clone)]
pub struct FileCommand {
    pub pattern: GlobMatcher,
    pub commands: Vec<String>,
}

pub struct FileCmdPopup {
    state: Cell<ListState>,
    registry: Vec<FileCommand>,
    queue: Queue,
    opened: Option<(FileCommand, PathBuf)>,
}

impl FileCmdPopup {
    pub fn new(queue: Queue, config: Rc<Config>) -> Self {
        let registry = config
            .special_commands
            .iter()
            .map(|(pattern, commands)| {
                let pat = Glob::new(&format!("**/{pattern}"))
                    .unwrap()
                    .compile_matcher();
                FileCommand {
                    pattern: pat,
                    commands: commands.clone(),
                }
            })
            .collect_vec();
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            queue,
            registry,
            state: state.into(),
            opened: None,
        }
    }

    pub fn open_for(&mut self, path: PathBuf) -> MatchState {
        self.state.get_mut().select(Some(0));
        let position = self
            .registry
            .iter()
            .position(|file_command| file_command.pattern.is_match(&path));
        if let Some(pos) = position {
            self.opened = Some((self.registry.remove(pos), path));
            MatchState::Matched
        } else {
            MatchState::NotMatched
        }
    }

    fn selected(&self) -> usize {
        let state = self.state.take();
        let selected = state.selected().expect("should have selected something");
        self.state.set(state);
        selected
    }

    fn select_next(&mut self) {
        let current = self.selected();
        let Some(opened) = &self.opened else {
            panic!("cannot call with no opened items")
        };
        if current == opened.0.commands.len() - 1 {
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

    fn select_first(&mut self) {
        self.state.get_mut().select(Some(0));
    }

    fn select_last(&mut self) {
        let Some(opened) = &self.opened else {
            panic!("cannot call with no opened items")
        };
        self.state
            .get_mut()
            .select(Some(opened.0.commands.len() - 1));
    }
}

impl Component for FileCmdPopup {
    fn visible(&self) -> bool {
        self.opened.is_some()
    }
    fn focused(&self) -> bool {
        true
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
                } => {
                    self.select_next();
                }
                KeyEvent {
                    code: KeyCode::Char('k'),
                    ..
                } => self.select_prev(),
                KeyEvent {
                    code: KeyCode::Esc | KeyCode::Char('q'),
                    ..
                } => {
                    let Some(opened) = self.opened.take() else {
                        unreachable!("checked at top of method");
                    };
                    self.registry.push(opened.0);
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } => {
                    let Some(opened) = self.opened.take() else {
                        unreachable!("checked at top of method");
                    };
                    let option = &opened.0.commands[self.selected()];
                    let replaced = option.replace("{}", &opened.1.display().to_string());
                    if replaced.contains("{...}") {
                        self.queue
                            .add(AppEvent::OpenInput(InputOperation::SpecialCommand(
                                replaced,
                            )));
                    } else {
                        self.queue.add(AppEvent::RunCommand(replaced));
                    }
                    self.registry.push(opened.0);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Drawable for FileCmdPopup {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let Some(opened) = &self.opened else {
            return Ok(());
        };

        let commands = opened
            .0
            .commands
            .iter()
            .map(|command| ListItem::new(command.as_str()))
            .collect_vec();
        let list = List::new(commands)
            .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen))
            .block(Block::default().borders(Borders::ALL));
        let area = ui::centered_rect_absolute(50, 10, area);
        f.render_widget(Clear, area);
        let mut state = self.state.take();
        f.render_stateful_widget(list, area, &mut state);
        self.state.set(state);

        Ok(())
    }
}
