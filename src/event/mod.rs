mod refresh;
pub use refresh::fs_watch;
mod crossterm_event;
pub use crossterm_event::crossterm_watch;

use anyhow::Error;
use crossterm::event::Event;

#[derive(Debug)]
pub enum EventType {
    RefreshFiletree,
    Crossterm(Event),
    Error(Error),
}
