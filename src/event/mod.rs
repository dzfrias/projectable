mod refresh;

pub use refresh::fs_watch;
mod crossterm_event;
use anyhow::Error;
use crossterm::event::Event;
pub use crossterm_event::crossterm_watch;

#[derive(Debug)]
pub enum ExternalEvent {
    RefreshFiletree,
    Crossterm(Event),
    Error(Error),
}
