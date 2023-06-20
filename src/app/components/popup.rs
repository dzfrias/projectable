use crate::{
    app::component::{Component, Drawable},
    config::{Config, Key},
    external_event::ExternalEvent,
    ui,
};
use anyhow::Result;
use crossterm::event::Event;
use easy_switch::switch;
use itertools::Itertools;
use std::{cell::Cell, rc::Rc};
use tui::{
    backend::Backend,
    layout::Rect,
    text::{Span, Spans},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub enum Preset {
    Help,
    RunningCommand,
    #[default]
    Nothing,
}

#[derive(Default)]
pub struct Popup {
    pub preset: Preset,
    scroll_y: Cell<u16>,
    config: Rc<Config>,
}

impl Popup {
    pub fn new(config: Rc<Config>) -> Self {
        Self {
            preset: Preset::default(),
            scroll_y: 0.into(),
            config,
        }
    }
}

impl Component for Popup {
    fn visible(&self) -> bool {
        self.preset != Preset::Nothing
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        if let ExternalEvent::Crossterm(Event::Key(key)) = ev {
            switch! { key;
                self.config.down => *self.scroll_y.get_mut() += 1,
                self.config.up => {
                    if self.scroll_y.get() != 0 {
                        *self.scroll_y.get_mut() -= 1;
                    }
                },
                self.config.all_up => *self.scroll_y.get_mut() = 0,
                self.config.all_down => *self.scroll_y.get_mut() = u16::MAX,
                self.config.quit => self.preset = Preset::Nothing,
                Key::esc() => self.preset = Preset::Nothing,
            }
        }

        Ok(())
    }
}

impl Drawable for Popup {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        let (text, title, height) = match self.preset {
            Preset::Help => {
                let keybinds = [
                    (self.config.open.to_string(), "Open file/toggle opened"),
                    (self.config.down.to_string(), "Move down"),
                    (self.config.up.to_string(), "Move up"),
                    (self.config.all_up.to_string(), "Go to bottom"),
                    (self.config.all_down.to_string(), "Go to top"),
                    (
                        self.config.filetree.down_three.to_string(),
                        "Move down by 3",
                    ),
                    (self.config.filetree.up_three.to_string(), "Move up by 3"),
                    (self.config.filetree.delete.to_string(), "Delete file"),
                    (self.config.filetree.new_file.to_string(), "Create new file"),
                    (
                        self.config.filetree.new_dir.to_string(),
                        "Create new directory",
                    ),
                    (self.config.filetree.search.to_string(), "Search"),
                    (self.config.filetree.clear.to_string(), "Clear search"),
                    (self.config.preview.up_key.to_string(), "Preview down"),
                    (self.config.preview.down_key.to_string(), "Preview up"),
                    (
                        self.config.filetree.diff_mode.to_string(),
                        "Toggle diff view",
                    ),
                    (
                        self.config.filetree.git_filter.to_string(),
                        "Filter for files with new git changes",
                    ),
                    (
                        self.config.filetree.show_dotfiles.to_string(),
                        "Show dotfiles",
                    ),
                    (self.config.filetree.exec_cmd.to_string(), "Execute command"),
                    (
                        self.config.filetree.special_command.to_string(),
                        "Execute special command",
                    ),
                    (
                        self.config.filetree.open_all.to_string(),
                        "Open all directories",
                    ),
                    (
                        self.config.filetree.close_all.to_string(),
                        "Close all directories",
                    ),
                    (
                        self.config.filetree.close_under.to_string(),
                        "Close all under directory",
                    ),
                    (
                        self.config.filetree.open_under.to_string(),
                        "Open all under directory",
                    ),
                    (
                        self.config.filetree.mark_selected.to_string(),
                        "Mark selected file",
                    ),
                    (self.config.filetree.rename.to_string(), "Rename/move file"),
                    (self.config.marks.open.to_string(), "Open marks window"),
                    (self.config.quit.to_string(), "Quit"),
                    (self.config.help.to_string(), "Open help window"),
                ];
                let longest_key_len = keybinds
                    .iter()
                    .map(|(key, _)| key.len())
                    .max()
                    .expect("should not be empty");
                (
                    keybinds
                        .into_iter()
                        .map(|(key, description)| {
                            Spans::from(vec![
                                Span::styled(
                                    // Pad based on longest key length
                                    format!("{:width$}", key, width = longest_key_len + 1),
                                    self.config.help_key_style.into(),
                                ),
                                Span::raw(description),
                            ])
                        })
                        .collect_vec(),
                    "Help",
                    35,
                )
            }
            Preset::RunningCommand => (
                vec![Spans::from(vec![Span::raw(
                    "Command in-progress. Press CTRL-C to quit",
                )])],
                "Command",
                3,
            ),
            Preset::Nothing => unreachable!("checked at top of method"),
        };

        let len = text.len() as u16;
        let area = ui::centered_rect_absolute(70, height, area);
        let scroll = {
            if len < area.height {
                self.scroll_y.set(0);
                0
            } else {
                self.scroll_y.take().clamp(0, len - area.height)
            }
        };
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(self.config.popup_border_style.into()),
            )
            .scroll((scroll, 0));
        self.scroll_y.set(scroll);

        f.render_widget(Clear, area);
        f.render_widget(paragraph, area);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;
    use test_log::test;

    #[test]
    fn visible_with_preset() {
        let mut popup = Popup::default();
        assert!(!popup.visible());
        popup.preset = Preset::Help;
        assert!(popup.visible());
    }

    #[test]
    fn q_resets() {
        let mut popup = Popup {
            preset: Preset::Help,
            ..Default::default()
        };

        let q = input_event!(KeyCode::Char('q'));
        popup.handle_event(&q).unwrap();
        assert_eq!(Preset::Nothing, popup.preset);
    }

    #[test]
    fn up_and_down_increment_scroll() {
        let mut popup = Popup {
            preset: Preset::Help,
            ..Default::default()
        };
        let [up, down] = input_events!(KeyCode::Char('k'), KeyCode::Char('j'));
        popup.handle_event(&down).unwrap();
        assert_eq!(1, popup.scroll_y.get());
        popup.handle_event(&up).unwrap();
        assert_eq!(0, popup.scroll_y.get());
    }

    #[test]
    fn g_and_shift_g_go_all_up_and_all_down() {
        let mut popup = Popup {
            preset: Preset::Help,
            ..Default::default()
        };
        let [all_up, all_down] =
            input_events!(KeyCode::Char('g'), KeyCode::Char('G'); KeyModifiers::SHIFT);
        popup.handle_event(&all_down).unwrap();
        assert_eq!(u16::MAX, popup.scroll_y.get());
        popup.handle_event(&all_up).unwrap();
        assert_eq!(0, popup.scroll_y.get());
    }
}
