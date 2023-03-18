use ansi_to_tui::IntoText;
use anyhow::{bail, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{cell::Cell, env, path::Path, process::Command};
use tui::{
    backend::Backend,
    layout::Rect,
    text::Text,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    app::component::{Component, Drawable},
    external_event::ExternalEvent,
};

pub struct PreviewFile {
    pub preview_command: String,
    contents: String,
    scrolls: u16,
    focused: bool,
    cache: Cell<Option<Text<'static>>>,
}

impl Default for PreviewFile {
    fn default() -> Self {
        Self {
            preview_command: if cfg!(target_os = "windows") {
                "type {}".to_owned()
            } else {
                "cat {}".to_owned()
            },
            contents: String::new(),
            scrolls: 0,
            focused: true,
            cache: None.into(),
        }
    }
}

impl PreviewFile {
    pub fn new(preview_command: String) -> Self {
        Self {
            contents: String::new(),
            preview_command,
            scrolls: 0,
            focused: true,
            cache: None.into(),
        }
    }

    pub fn preview_file(&mut self, file: impl AsRef<Path>) -> Result<()> {
        if self.preview_command.is_empty() {
            bail!("should have command");
        }
        self.scrolls = 0;
        // Cache has to be reset
        self.cache = None.into();
        let replaced = {
            let replacement = if cfg!(target_os = "windows") {
                file.as_ref().display().to_string().replace(" ", "\\` ")
            } else {
                format!("'{}'", &file.as_ref().display().to_string())
            };

            self.preview_command.replace("{}", &replacement)
        };
        self.contents = if cfg!(target_os = "windows") {
            let out = Command::new("cmd").arg("/C").arg(&replaced).output()?;
            let output = if out.stdout.is_empty() && !out.stderr.is_empty() {
                out.stderr
            } else {
                out.stdout
            };
            String::from_utf8_lossy(&output).to_string()
        } else {
            let out = Command::new(env::var("SHELL").unwrap_or("sh".to_owned()))
                .arg("-c")
                .arg(&replaced)
                .output()?;
            if out.stdout.is_empty() && !out.stderr.is_empty() {
                String::from_utf8_lossy(&out.stderr).to_string()
            } else {
                String::from_utf8_lossy(&out.stdout).to_string()
            }
        };
        Ok(())
    }
}

impl Component for PreviewFile {
    fn focus(&mut self, focus: bool) {
        self.focused = focus;
    }
    fn focused(&self) -> bool {
        self.focused
    }

    fn visible(&self) -> bool {
        true
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.focused {
            return Ok(());
        }

        const BIG_SCROLL_AMOUNT: u16 = 10;
        if let ExternalEvent::Crossterm(Event::Key(key)) = ev {
            match key {
                KeyEvent {
                    code: KeyCode::Char('d'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    let num_lines = self.contents.lines().count();
                    if ((self.scrolls + BIG_SCROLL_AMOUNT) as usize) < num_lines {
                        self.scrolls += BIG_SCROLL_AMOUNT;
                    }
                }
                KeyEvent {
                    code: KeyCode::Char('K'),
                    ..
                } => {
                    if self.scrolls != 0 {
                        self.scrolls -= 1;
                    }
                }
                KeyEvent {
                    code: KeyCode::Char('J'),
                    ..
                } => {
                    let num_lines = self.contents.lines().count();
                    if ((self.scrolls + 1) as usize) < num_lines {
                        self.scrolls += 1;
                    }
                }
                KeyEvent {
                    code: KeyCode::Char('u'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    if self.scrolls >= BIG_SCROLL_AMOUNT {
                        self.scrolls -= BIG_SCROLL_AMOUNT;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

impl Drawable for PreviewFile {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let text = if let Some(cache) = self.cache.take() {
            cache
        } else {
            // Remove bold modifier, it was causing problems
            self.contents.replace("[1m", "").into_text()?
        };
        self.cache.set(Some(text.clone()));

        let paragraph = Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL))
            .scroll((self.scrolls, 0));
        f.render_widget(paragraph, area);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{prelude::*, TempDir};

    fn preview_default() -> String {
        let program = if cfg!(target_os = "windows") {
            "type {}"
        } else {
            "cat {}"
        };
        program.to_owned()
    }

    #[test]
    fn can_get_file_contents() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        temp_dir
            .child("test.txt")
            .write_str("should be previewed")
            .unwrap();
        let path = temp_dir.path().to_owned();
        let mut previewer = PreviewFile::new(preview_default());
        previewer
            .preview_file(path.join("test.txt"))
            .expect("preview should work");
        assert_eq!("should be previewed".to_owned(), previewer.contents);
    }

    #[test]
    fn does_not_work_with_zero_args() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        temp_dir
            .child("test.txt")
            .write_str("should be previewed")
            .unwrap();
        let mut previewer = PreviewFile::new("".to_owned());
        assert!(previewer.preview_file(temp_dir.join("test.txt")).is_err());
    }

    #[test]
    fn works_with_one_arg() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        temp_dir
            .child("test.txt")
            .write_str("should be previewed")
            .unwrap();
        let mut previewer =
            PreviewFile::new(preview_default().strip_suffix(" {}").unwrap().to_owned());
        assert!(previewer.preview_file(temp_dir.join("test.txt")).is_ok());
    }

    #[test]
    fn works_with_file_with_spaces() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        let child = temp_dir.child("hello world");
        let path = child.path();
        child.write_str("should be previewed").unwrap();

        let mut previewer = PreviewFile::new(preview_default());
        previewer.preview_file(path).expect("preview should work");
        assert_eq!("should be previewed", previewer.contents);
    }
}
