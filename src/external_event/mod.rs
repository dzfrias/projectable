mod crossterm_event;
mod refresh;

use anyhow::Error;
use crossterm::event::Event;
pub use crossterm_event::crossterm_watch;
pub use refresh::fs_watch;
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
    Error(Error),
}
