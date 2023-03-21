use std::cell::Cell;

use crate::{
    app::component::{Component, Drawable},
    external_event::ExternalEvent,
    ui,
};
use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent};
use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub enum Preset {
    Help,
    #[default]
    Nothing,
}

#[derive(Default)]
pub struct Popup {
    pub preset: Preset,
    scroll_y: Cell<u16>,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            preset: Preset::default(),
            scroll_y: 0.into(),
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

        if let ExternalEvent::Crossterm(Event::Key(KeyEvent { code, .. })) = ev {
            match code {
                KeyCode::Char('q') | KeyCode::Esc => self.preset = Preset::Nothing,
                KeyCode::Char('k') => {
                    if self.scroll_y.get() != 0 {
                        *self.scroll_y.get_mut() -= 1;
                    }
                }
                KeyCode::Char('j') => {
                    *self.scroll_y.get_mut() += 1;
                }
                KeyCode::Char('g') => {
                    *self.scroll_y.get_mut() = 0;
                }
                KeyCode::Char('G') => {
                    *self.scroll_y.get_mut() = u16::MAX;
                }
                _ => {}
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

        let (text, title, len) = match self.preset {
            Preset::Help => {
                let keybinds = [
                    ("Enter", "Open file/toggle opened"),
                    ("j", "Move down"),
                    ("k", "Move up"),
                    ("g", "Go to bottom"),
                    ("G", "Go to top"),
                    ("Ctrl-n", "Move down by 3"),
                    ("Ctrl-p", "Move up by 3"),
                    ("d", "Delete file"),
                    ("n", "Create new file"),
                    ("N", "Create new directory"),
                    ("/", "Search"),
                    ("\\", "Clear search"),
                    ("Ctrl-d", "Preview down"),
                    ("Ctrl-u", "Preview up"),
                    ("t", "Toggle diff view"),
                    ("T", "Filter for files with new git changes"),
                    ("e", "Execute command"),
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
                                    Style::default()
                                        .fg(Color::Cyan)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::raw(description),
                            ])
                        })
                        .collect::<Vec<Spans>>(),
                    "Help",
                    keybinds.len() as u16,
                )
            }
            Preset::Nothing => unreachable!("checked at top of method"),
        };

        let area = ui::centered_rect_absolute(70, 35, area);
        let scroll = {
            if len < area.height {
                self.scroll_y.set(0);
                0
            } else {
                self.scroll_y.take().clamp(0, len - area.height)
            }
        };
        let paragraph = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(title))
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

    #[test]
    fn visible_with_preset() {
        let mut popup = Popup::default();
        assert!(!popup.visible());
        popup.preset = Preset::Help;
        assert!(popup.visible());
    }

    #[test]
    fn q_resets() {
        let mut popup = Popup::default();
        popup.preset = Preset::Help;

        let q = input_event!(KeyCode::Char('q'));
        popup.handle_event(&q).unwrap();
        assert_eq!(Preset::Nothing, popup.preset);
    }

    #[test]
    fn up_and_down_increment_scroll() {
        let mut popup = Popup::default();
        popup.preset = Preset::Help;
        let [up, down] = input_events!(KeyCode::Char('k'), KeyCode::Char('j'));
        popup.handle_event(&down).unwrap();
        assert_eq!(1, popup.scroll_y.get());
        popup.handle_event(&up).unwrap();
        assert_eq!(0, popup.scroll_y.get());
    }

    #[test]
    fn g_and_shift_g_go_all_up_and_all_down() {
        let mut popup = Popup::default();
        popup.preset = Preset::Help;
        let [all_up, all_down] = input_events!(KeyCode::Char('g'), KeyCode::Char('G'));
        popup.handle_event(&all_down).unwrap();
        assert_eq!(u16::MAX, popup.scroll_y.get());
        popup.handle_event(&all_up).unwrap();
        assert_eq!(0, popup.scroll_y.get());
    }
}
