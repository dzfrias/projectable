use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
    ui::{ParagraphState, ScrollParagraph},
};
use ansi_to_tui::IntoText;
use anyhow::{bail, Context, Result};
use crossterm::event::{Event, MouseEventKind};
#[cfg(not(target_os = "windows"))]
use duct::cmd;
use easy_switch::switch;
use log::trace;
#[cfg(not(target_os = "windows"))]
use std::env;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::Command;
use std::{cell::Cell, collections::VecDeque, path::Path, rc::Rc};
use tui::{
    backend::Backend,
    layout::Rect,
    widgets::{Block, Borders},
    Frame,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
enum ScrollDirection {
    Down,
    Up,
}

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
struct Scroll {
    direction: ScrollDirection,
    x: u16,
    y: u16,
}

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
    focused: bool,
    config: Rc<Config>,
    state: Cell<ParagraphState>,
    scrolls: Cell<VecDeque<Scroll>>,
}

impl Default for PreviewFile {
    fn default() -> Self {
        Self {
            contents: String::new(),
            focused: true,
            mode: Mode::default(),
            config: Rc::new(Config::default()),
            git_cmd: "git diff {}".to_owned(),
            state: ParagraphState::default().into(),
            scrolls: VecDeque::new().into(),
        }
    }
}

impl PreviewFile {
    pub fn new() -> Self {
        Self {
            contents: String::new(),
            focused: true,
            mode: Mode::default(),
            config: Rc::new(Config::default()),
            git_cmd: "git diff {}".to_owned(),
            state: ParagraphState::default().into(),
            scrolls: VecDeque::new().into(),
        }
    }

    pub fn with_config(config: Rc<Config>) -> Self {
        Self {
            config: Rc::clone(&config),
            git_cmd: config
                .preview
                .git_pager
                .as_ref()
                .map_or("git diff {}".to_owned(), |cmd| {
                    format!("git diff {{}} | {}", cmd)
                }),
            ..Self::new()
        }
    }

    pub fn preview_file(&mut self, file: impl AsRef<Path>) -> Result<()> {
        if self.config.preview.preview_cmd.is_empty() || self.git_cmd.is_empty() {
            bail!("should have command");
        }
        self.state.get_mut().reset();
        let replaced = {
            #[cfg(target_os = "windows")]
            let replacement = format!("\"{}\"", file.as_ref().display());
            #[cfg(not(target_os = "windows"))]
            let replacement = format!("'{}'", file.as_ref().display());

            if self.mode == Mode::Preview {
                self.config.preview.preview_cmd.replace("{}", &replacement)
            } else {
                self.git_cmd.replace("{}", &replacement)
            }
        };

        #[cfg(target_os = "windows")]
        let out = {
            let out = Command::new("cmd.exe")
                // See https://github.com/rust-lang/rust/issues/92939
                .raw_arg(&format!("/C {replaced}"))
                .output()
                .with_context(|| format!("problem running preview command with {replaced}"))?;
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        #[cfg(not(target_os = "windows"))]
        let out = cmd!(
            env::var("SHELL").unwrap_or("sh".to_owned()),
            "-c",
            &replaced
        )
        .unchecked()
        .stderr_to_stdout()
        .read()
        .with_context(|| format!("problem running preview command with {replaced}"))?;

        trace!("ran preview command: \"{replaced}\"");
        self.contents = out;
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

        if let ExternalEvent::Crossterm(event) = ev {
            match event {
                Event::Key(key) => {
                    switch! { key;
                        self.config.preview.down_key => self.state.get_mut().down_by(self.config.preview.scroll_amount),
                        self.config.preview.up_key => self.state.get_mut().up_by(self.config.preview.scroll_amount),
                    }
                }
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        self.scrolls.get_mut().push_front(Scroll {
                            direction: ScrollDirection::Down,
                            x: mouse.column,
                            y: mouse.row,
                        });
                    }
                    MouseEventKind::ScrollUp => {
                        self.scrolls.get_mut().push_back(Scroll {
                            direction: ScrollDirection::Up,
                            x: mouse.column,
                            y: mouse.row,
                        });
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Ok(())
    }
}

impl Drawable for PreviewFile {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let text = self.contents.into_text()?;
        let paragraph = ScrollParagraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Preview")
                    .border_style(self.config.preview.border_color.into()),
            )
            .bar_style(self.config.preview.scroll_bar_color.into())
            .unreached_bar_style(self.config.preview.unreached_bar_color.into());
        let mut state = self.state.take();
        let mut scrolls = self.scrolls.take();
        while let Some(Scroll { direction, x, y }) = scrolls.pop_front() {
            let mouse_rect = Rect::new(x, y, 1, 1);
            if area.intersects(mouse_rect) {
                match direction {
                    ScrollDirection::Up => state.up(),
                    ScrollDirection::Down => state.down(),
                }
            }
        }
        f.render_stateful_widget(paragraph, area, &mut state);
        self.state.set(state);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{prelude::*, TempDir};
    use collect_all::collect;
    use crossterm::event::{KeyModifiers, MouseEvent};
    use test_log::test;

    fn preview_default() -> String {
        #[cfg(target_os = "windows")]
        let program = "type {}";
        #[cfg(not(target_os = "windows"))]
        let program = "cat {}";

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
    fn mouse_inputs_are_stored_in_queue() {
        let mut previewer = PreviewFile::default();
        let events = [
            ExternalEvent::Crossterm(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 22,
                row: 22,
                modifiers: KeyModifiers::NONE,
            })),
            ExternalEvent::Crossterm(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 1,
                row: 2,
                modifiers: KeyModifiers::NONE,
            })),
        ];
        for event in events {
            assert!(previewer.handle_event(&event).is_ok());
        }
        assert_eq!(
            collect![VecDeque<Scroll>:
                Scroll {
                    direction: ScrollDirection::Down,
                    x: 22,
                    y: 22,
                },
                Scroll {
                    direction: ScrollDirection::Up,
                    x: 1,
                    y: 2,
                }
            ],
            previewer.scrolls.take()
        );
    }
}
