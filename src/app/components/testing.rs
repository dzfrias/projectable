#[allow(unused_macros)]
macro_rules! input_event {
    ($key:expr) => {{
        use ::crossterm::event::{
            Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };
        ExternalEvent::Crossterm(Event::Key(KeyEvent {
            code: $key,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }))
    }};
    ($key:expr, $mods:expr) => {{
        use ::crossterm::event::{
            Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
        };
        ExternalEvent::Crossterm(Event::Key(KeyEvent {
            code: $key,
            modifiers: $mods,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }))
    }};
}
#[allow(unused_imports)]
pub(crate) use input_event;

#[allow(unused_macros)]
macro_rules! input_events {
    ($($key:expr),+) => {
        {
            [$(input_event!($key)),+]
        }
    };
}
#[allow(unused_imports)]
pub(crate) use input_events;
