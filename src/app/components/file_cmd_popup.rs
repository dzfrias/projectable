use crate::{
    app::component::{Component, Drawable},
    config::{Config, Key},
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use crossterm::event::Event;
use easy_switch::switch;
use globset::{Glob, GlobMatcher};
use itertools::Itertools;
use std::{cell::Cell, path::PathBuf, rc::Rc};

use tui::{
    backend::Backend,
    layout::Rect,
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
    config: Rc<Config>,
}

impl Default for FileCmdPopup {
    fn default() -> Self {
        Self::new(Queue::new(), Config::default().into())
    }
}

impl FileCmdPopup {
    pub fn new(queue: Queue, config: Rc<Config>) -> Self {
        let registry = config
            .special_commands
            .iter()
            .map(|(pattern, commands)| {
                // Prefixed with ** to work with absolute paths
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
            config,
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

    fn close(&mut self) {
        let Some(opened) = self.opened.take() else {
            return;
        };
        self.registry.push(opened.0);
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
            switch! { key;
                self.config.down => self.select_next(),
                self.config.up => self.select_prev(),
                self.config.all_up => self.select_first(),
                self.config.all_down => self.select_last(),
                self.config.quit => self.close(),
                Key::esc() => self.close(),
                self.config.open => {
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
            .highlight_style(self.config.selected.into())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.config.popup_border_style.into())
                    .title("Special Commands"),
            );
        let area = ui::centered_rect_absolute(50, 10, area);
        f.render_widget(Clear, area);
        let mut state = self.state.take();
        f.render_stateful_widget(list, area, &mut state);
        self.state.set(state);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;
    use collect_all::collect;
    use test_log::test;

    fn test_popup() -> FileCmdPopup {
        let config = Config {
            special_commands: collect![_:
                ("*".to_owned(), vec!["command {}".to_owned(), "command2 {} {...}".to_owned(), "command3".to_owned()])
            ],
            ..Default::default()
        };
        let mut popup = FileCmdPopup::new(Queue::new(), config.into());
        let path = "test.txt".into();
        popup.open_for(path);
        popup
    }

    #[test]
    fn starts_with_first_selected() {
        let popup = FileCmdPopup::default();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn silent_when_file_does_not_match() {
        let path = "test.txt".into();
        let mut popup = FileCmdPopup::default();
        let state = popup.open_for(path);
        assert_eq!(MatchState::NotMatched, state);
    }

    #[test]
    fn can_open_for_file() {
        let config = Config {
            special_commands: collect![_:
                ("*".to_owned(), vec!["command".to_owned()]),
                ("not_there.txt".to_owned(), vec!["should_not_be_here".to_owned()])
            ],
            ..Default::default()
        };
        let mut popup = FileCmdPopup::new(Queue::new(), config.into());
        let path = "test.txt".into();
        let state = popup.open_for(path);
        assert_eq!(MatchState::Matched, state);
        assert_eq!(PathBuf::from("test.txt"), popup.opened.as_ref().unwrap().1);
        assert_eq!(vec!["command".to_owned()], popup.opened.unwrap().0.commands);
    }

    #[test]
    fn selecting_prev_cannot_go_below_zero() {
        let mut popup = FileCmdPopup::default();
        popup.select_prev();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn selecting_next_cannot_go_above_opened_amount() {
        let mut popup = test_popup();
        for _ in 0..10 {
            popup.select_next();
        }
        assert_eq!(2, popup.selected());
    }

    #[test]
    fn can_select_last() {
        let mut popup = test_popup();
        popup.select_last();
        assert_eq!(2, popup.selected());
    }

    #[test]
    fn can_select_first() {
        let mut popup = test_popup();
        popup.select_next();
        popup.select_first();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn visible_with_work() {
        let popup = test_popup();
        assert!(popup.visible());
    }

    #[test]
    fn quitting_properly_restores_registry() {
        let mut popup = test_popup();
        assert!(popup.registry.is_empty());
        let event = input_event!(KeyCode::Char('q'));
        popup.handle_event(&event).unwrap();
        assert_eq!(1, popup.registry.len());
    }

    #[test]
    fn confirming_properly_restores_registry() {
        let mut popup = test_popup();
        assert!(popup.registry.is_empty());
        let event = input_event!(KeyCode::Enter);
        popup.handle_event(&event).unwrap();
        assert_eq!(1, popup.registry.len());
    }

    #[test]
    fn confirming_sends_run_command_event_with_interpolated_path() {
        let mut popup = test_popup();
        let event = input_event!(KeyCode::Enter);
        popup.handle_event(&event).unwrap();
        assert!(popup
            .queue
            .contains(&AppEvent::RunCommand("command test.txt".to_owned())));
    }

    #[test]
    fn confirming_with_search_interpolation_opens_search_box() {
        let mut popup = test_popup();
        popup.select_next();
        let event = input_event!(KeyCode::Enter);
        popup.handle_event(&event).unwrap();
        assert!(popup
            .queue
            .contains(&AppEvent::OpenInput(InputOperation::SpecialCommand(
                "command2 test.txt {...}".to_owned()
            ))));
    }
}
