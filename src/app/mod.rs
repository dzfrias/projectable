pub mod component;
mod components;

use self::component::{Component, Drawable};
pub use self::components::*;
use crate::{
    config::Config,
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
};
use anyhow::Result;
use crossterm::event::Event;
use easy_switch::switch;
use itertools::Itertools;
use log::{info, warn};
use rust_search::SearchBuilder;
use std::{
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders},
    Frame,
};
use tui_logger::{TuiLoggerLevelOutput, TuiLoggerWidget, TuiWidgetState};

/// Event that is sent back up to main.rs
#[derive(Debug)]
pub enum TerminalEvent {
    OpenFile(PathBuf),
}

pub struct App {
    tree: Filetree,
    path: PathBuf,
    should_quit: bool,
    queue: Queue,
    pending: PendingPopup,
    input_box: InputBox,
    previewer: PreviewFile,
    text_popup: Popup,
    config: Rc<Config>,
}

impl App {
    pub fn new(path: impl AsRef<Path>, cwd: impl AsRef<Path>, config: Rc<Config>) -> Result<Self> {
        let queue = Queue::new();
        let mut tree = Filetree::from_dir_with_config(&path, queue.clone(), Rc::clone(&config))?;
        tree.open_path(cwd)?;
        Ok(App {
            path: path.as_ref().to_path_buf(),
            tree,
            should_quit: false,
            pending: PendingPopup::new(queue.clone()),
            input_box: InputBox::new(queue.clone()),
            previewer: PreviewFile::with_config(Rc::clone(&config)),
            text_popup: Popup::default(),
            config: Rc::clone(&config),
            queue,
        })
    }

    /// Returns None if no events should be sent to the terminal
    pub fn update(&mut self) -> Result<Option<TerminalEvent>> {
        while let Some(app_event) = self.queue.pop() {
            // Handle events from queue
            match app_event {
                AppEvent::OpenPopup(operation) => self.pending.operation = operation,
                AppEvent::DeleteFile(path) => {
                    if path.is_file() {
                        fs::remove_file(&path)?;
                        info!(" deleted file \"{}\"", path.display());
                    } else {
                        fs::remove_dir_all(&path)?;
                        info!(" deleted directory \"{}\"", path.display());
                    }
                    self.tree.partial_refresh(RefreshData::Delete(path))?;
                    self.queue.add(AppEvent::PreviewFile(
                        self.tree.get_selected().unwrap().path().to_owned(),
                    ));
                }
                AppEvent::OpenFile(path) => {
                    info!(" opening file \"{}\"", path.display());
                    return Ok(Some(TerminalEvent::OpenFile(path)));
                }
                AppEvent::OpenInput(op) => self.input_box.operation = op,
                AppEvent::NewFile(path) => {
                    File::create(&path)?;
                    info!(" created file \"{}\"", path.display());
                    self.tree.partial_refresh(RefreshData::Add(path))?;
                }
                AppEvent::NewDir(path) => {
                    fs::create_dir(&path)?;
                    info!(" created directory \"{}\"", path.display());
                    self.tree.partial_refresh(RefreshData::Add(path))?;
                }
                AppEvent::PreviewFile(path) => self.previewer.preview_file(path)?,
                AppEvent::TogglePreviewMode => self.previewer.toggle_mode(),
                AppEvent::RunCommand(cmd) => {
                    if cfg!(target_os = "windows") {
                        let output = Command::new("cmd").arg("/C").arg(&cmd).output()?;
                        info!("\n{}", String::from_utf8_lossy(&output.stdout));
                    } else {
                        let output = Command::new(env::var("SHELL").unwrap_or("sh".to_owned()))
                            .arg("-c")
                            .arg(&cmd)
                            .output()?;
                        info!("\n{}", String::from_utf8_lossy(&output.stdout));
                    }
                }
                AppEvent::SearchFiles(search) => {
                    info!(" searching for: \"{}\"", search);
                    let results = SearchBuilder::default()
                        .location(&self.path)
                        .search_input(search)
                        .ignore_case()
                        .hidden()
                        .build()
                        .map_into()
                        .collect_vec();
                    if results.is_empty() {
                        warn!(" no files found when searching");
                    }
                    self.tree.only_include(results.as_ref())?;
                }
            }
        }

        Ok(None)
    }

    pub fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        let popup_open =
            self.pending.visible() || self.input_box.visible() || self.text_popup.visible();
        // Do not give the Filetree or previewer focus if there are any popups open
        self.tree.focus(!popup_open);
        self.previewer.focus(!popup_open);

        self.pending.handle_event(ev)?;
        self.input_box.handle_event(ev)?;
        self.tree.handle_event(ev)?;
        self.previewer.handle_event(ev)?;
        self.text_popup.handle_event(ev)?;

        if popup_open {
            return Ok(());
        }
        if let ExternalEvent::Crossterm(Event::Key(key)) = ev {
            switch! { key;
                self.config.quit => self.should_quit = true,
                self.config.help => self.text_popup.preset = Preset::Help,
            }
        }
        Ok(())
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drawable for App {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let main_layout = Layout::default()
            .direction(Direction::Horizontal)
            .horizontal_margin(1)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);
        let left_hand_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
            .split(main_layout[0]);

        let logger = TuiLoggerWidget::default()
            .style_error(Style::default().fg(Color::Red))
            .style_debug(Style::default().fg(Color::Green))
            .style_warn(Style::default().fg(Color::Yellow))
            .style_trace(Style::default().fg(Color::Magenta))
            .style_info(Style::default().fg(Color::Cyan))
            .output_level(Some(TuiLoggerLevelOutput::Long))
            .output_target(false)
            .output_file(false)
            .output_line(false)
            .block(Block::default().borders(Borders::ALL).title("Log"))
            .state(&TuiWidgetState::new());

        self.tree.draw(f, left_hand_layout[0])?;
        f.render_widget(logger, left_hand_layout[1]);
        self.previewer.draw(f, main_layout[1])?;
        self.pending.draw(f, area)?;
        self.input_box.draw(f, area)?;
        self.text_popup.draw(f, area)?;

        Ok(())
    }
}
