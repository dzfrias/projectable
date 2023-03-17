mod crossterm_event;
mod refresh;

use anyhow::Error;
use crossterm::event::Event;
pub use crossterm_event::crossterm_watch;
pub use refresh::fs_watch;

#[derive(Debug)]
pub enum ExternalEvent {
    RefreshFiletree,
    /// Wrapper for crossterm events
    Crossterm(Event),
    Error(Error),
}
