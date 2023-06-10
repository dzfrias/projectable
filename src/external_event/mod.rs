mod crossterm_event;
mod refresh;
mod run_cmd;

use anyhow::Error;
use crossterm::event::Event;
pub use crossterm_event::*;
pub use refresh::fs_watch;
pub use run_cmd::*;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RefreshData {
    Delete(PathBuf),
    Add(PathBuf),
}

#[derive(Debug)]
pub enum ExternalEvent {
    RefreshFiletree,
    PartialRefresh(Vec<RefreshData>),
    /// Wrapper for crossterm events
    Crossterm(Event),
    CommandOutput(String),
    Error(Error),
}
