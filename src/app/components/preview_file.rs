use ansi_to_tui::IntoText;
use anyhow::{bail, Result};
use crossterm::event::Event;
use std::{cell::Cell, env, path::Path, process::Command, rc::Rc};
use tui::{
    backend::Backend,
    layout::Rect,
    text::Text,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
};

#[derive(Default, PartialEq, Eq, Clone, Debug)]
enum Mode {
    #[default]
    Preview,
    Diff,
}

pub struct PreviewFile {
    git_cmd: String,
    mode: Mode,
    contents: String,
    scrolls: u16,
    focused: bool,
    cache: Cell<Option<Text<'static>>>,
    config: Rc<Config>,
}

impl Default for PreviewFile {
    fn default() -> Self {
        Self {
            contents: String::new(),
            scrolls: 0,
            focused: true,
            cache: None.into(),
            mode: Mode::default(),
            config: Rc::new(Config::default()),
            git_cmd: "git diff {}".to_owned(),
        }
    }
}

impl PreviewFile {
    pub fn new() -> Self {
        Self {
            contents: String::new(),
            scrolls: 0,
            focused: true,
            cache: None.into(),
            mode: Mode::default(),
            config: Rc::new(Config::default()),
            git_cmd: "git diff {}".to_owned(),
        }
    }

    pub fn with_config(config: Rc<Config>) -> Self {
        Self {
            contents: String::new(),
            scrolls: 0,
            focused: true,
            cache: None.into(),
            mode: Mode::default(),
            config: Rc::clone(&config),
            git_cmd: config
                .preview
                .git_pager
                .as_ref()
                .map(|cmd| format!("git diff {{}} | {}", cmd))
                .unwrap_or("git diff {}".to_owned()),
        }
    }

    pub fn preview_file(&mut self, file: impl AsRef<Path>) -> Result<()> {
        if self.config.preview.preview_cmd.is_empty() || self.git_cmd.is_empty() {
            bail!("should have command");
        }
        self.scrolls = 0;
        // Cache has to be reset
        self.cache = None.into();
        let replaced = {
            let replacement = if cfg!(target_os = "windows") {
                file.as_ref().display().to_string()
            } else {
                format!("'{}'", &file.as_ref().display().to_string())
            };

            if self.mode == Mode::Preview {
                self.config.preview.preview_cmd.replace("{}", &replacement)
            } else {
                self.git_cmd.replace("{}", &replacement)
            }
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

    pub fn toggle_mode(&mut self) {
        if self.mode == Mode::Preview {
            self.mode = Mode::Diff;
        } else {
            self.mode = Mode::Preview;
        }
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
            if self.config.preview.down_key == key {
                let num_lines = self.contents.lines().count();
                if ((self.scrolls + BIG_SCROLL_AMOUNT) as usize) < num_lines {
                    self.scrolls += BIG_SCROLL_AMOUNT;
                } else {
                    self.scrolls = num_lines as u16 - 1;
                }
            } else if self.config.preview.up_key == key {
                if self.scrolls >= BIG_SCROLL_AMOUNT {
                    self.scrolls -= BIG_SCROLL_AMOUNT;
                } else {
                    self.scrolls = 0;
                }
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
            self.contents.into_text()?
        };
        self.cache.set(Some(text.clone()));

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.config.preview.border_color.into()),
            )
            .scroll((self.scrolls, 0));
        f.render_widget(paragraph, area);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
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

    #[cfg(windows)]
    const LINE_ENDING: &'static str = "\r\n";
    #[cfg(not(windows))]
    const LINE_ENDING: &'static str = "\n";

    #[test]
    fn can_get_file_contents() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        temp_dir
            .child("test.txt")
            .write_str("should be previewed")
            .unwrap();
        let path = temp_dir.path().to_owned();
        let mut previewer = PreviewFile::default();
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

        let mut config = Config::default();
        config.preview.preview_cmd = String::new();
        config.preview.git_pager = Some("test".to_owned());
        let mut previewer = PreviewFile::with_config(Rc::new(config));
        assert!(previewer.preview_file(temp_dir.join("test.txt")).is_err());
        let mut config = Config::default();
        config.preview.preview_cmd = String::new();
        config.preview.git_pager = Some("test".to_owned());
        let mut previewer = PreviewFile::with_config(Rc::new(config));
        assert!(previewer.preview_file(temp_dir.join("test.txt")).is_err());
    }

    #[test]
    fn works_with_one_arg() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        temp_dir
            .child("test.txt")
            .write_str("should be previewed")
            .unwrap();
        let mut config = Config::default();
        config.preview.preview_cmd = preview_default().strip_suffix(" {}").unwrap().to_owned();
        let mut previewer = PreviewFile::with_config(Rc::new(config));
        assert!(previewer.preview_file(temp_dir.join("test.txt")).is_ok());
    }

    #[test]
    // FIX: Does not work on windows yet
    #[cfg(not(target_os = "windows"))]
    fn works_with_file_with_spaces() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        let child = temp_dir.child("hello world");
        child.write_str("should be previewed").unwrap();

        let mut previewer = PreviewFile::default();
        previewer
            .preview_file(child.path())
            .expect("preview should work");
        assert_eq!("should be previewed", previewer.contents);
    }

    #[test]
    fn cannot_scroll_above_file() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        let child = temp_dir.child("hello world");
        child
            .write_str(&format!("line{}another", LINE_ENDING))
            .unwrap();

        let big_up = input_event!(KeyCode::Char('u'); KeyModifiers::CONTROL);

        let mut previewer = PreviewFile::default();
        previewer
            .preview_file(child.path())
            .expect("preview should work");
        previewer.handle_event(&big_up).expect("should handle");
        assert_eq!(0, previewer.scrolls);
    }

    #[test]
    // FIX: Does not work on windows yet
    #[cfg(not(target_os = "windows"))]
    fn cannot_scroll_below_file() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        let child = temp_dir.child("hello world");
        child
            .write_str(&format!("line{}another", LINE_ENDING))
            .unwrap();

        let big_down = input_event!(KeyCode::Char('d'); KeyModifiers::CONTROL);

        let mut previewer = PreviewFile::default();
        previewer
            .preview_file(child.path())
            .expect("preview should work");
        previewer.handle_event(&big_down).expect("should handle");
        assert_eq!(1, previewer.scrolls);
    }

    #[test]
    // FIX: Does not work on windows yet
    #[cfg(not(target_os = "windows"))]
    fn big_up_goes_as_far_as_possible() {
        let temp_dir = TempDir::new().expect("should be able to make temp dir");
        let child = temp_dir.child("hello world");
        child
            .write_str(&format!("line{}another", LINE_ENDING))
            .unwrap();

        let big_up = input_event!(KeyCode::Char('u'); KeyModifiers::CONTROL);

        let mut previewer = PreviewFile::default();
        previewer
            .preview_file(child.path())
            .expect("preview should work");
        previewer.handle_event(&big_up).expect("should handle");
        assert_eq!(0, previewer.scrolls);
    }
}
