use crate::event::ExternalEvent;
use anyhow::Result;
use tui::{backend::Backend, layout::Rect, Frame};

#[derive(Debug, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Hidden,
    Visible,
}

pub trait Drawable {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()>;
}

pub trait Component {
    fn visible(&self) -> bool;

    fn focus(&mut self, _focus: bool) {}
    fn focused(&self) -> bool {
        true
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()>;
}
