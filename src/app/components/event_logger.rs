use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
    logger::{EventLoggerSubscriber, EVENT_LOGGER},
    ui::{ParagraphState, ScrollParagraph},
};
use anyhow::Result;
use crossterm::event::{Event, MouseEventKind};
use log::Level;
use std::{
    cell::Cell,
    collections::VecDeque,
    rc::Rc,
    sync::{Arc, RwLock, RwLockReadGuard},
};
use tui::{
    backend::Backend,
    layout::Rect,
    text::Text,
    widgets::{Block, Borders},
    Frame,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
enum ScrollDirection {
    Down,
    Up,
}

#[derive(Debug)]
struct ScrollEvent {
    direction: ScrollDirection,
    point: (u16, u16),
}

pub struct LogReceiver {
    messages: RwLock<Vec<(String, Level)>>,
}

pub struct EventLogger {
    focused: bool,
    scrolls: Cell<VecDeque<ScrollEvent>>,
    state: Cell<ParagraphState>,
    locked: Cell<bool>,
    receiver: Arc<LogReceiver>,
    config: Rc<Config>,
}

impl LogReceiver {
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(Vec::new()),
        }
    }

    pub fn messages(&self) -> RwLockReadGuard<'_, Vec<(String, Level)>> {
        self.messages.read().unwrap()
    }
}

impl EventLoggerSubscriber for LogReceiver {
    fn receive(&self, message: &str, level: Level) {
        self.messages
            .write()
            .unwrap()
            .push((message.to_owned(), level));
    }
}

impl EventLogger {
    pub fn new(config: Rc<Config>) -> Self {
        let receiver = Arc::new(LogReceiver::new());
        EVENT_LOGGER.subscribe(receiver.clone());
        Self {
            focused: true,
            scrolls: VecDeque::new().into(),
            state: ParagraphState::default().into(),
            locked: true.into(),
            receiver: Arc::clone(&receiver),
            config,
        }
    }
}

impl Component for EventLogger {
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
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollDown => {
                        self.scrolls.get_mut().push_front(ScrollEvent {
                            direction: ScrollDirection::Down,
                            point: (mouse.column, mouse.row),
                        });
                    }
                    MouseEventKind::ScrollUp => {
                        self.scrolls.get_mut().push_back(ScrollEvent {
                            direction: ScrollDirection::Up,
                            point: (mouse.column, mouse.row),
                        });
                        self.locked.set(false);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Ok(())
    }
}

impl Drawable for EventLogger {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        let messages = self.receiver.messages();
        let texts = messages.iter().map(|(s, level)| {
            let style = match level {
                Level::Trace => self.config.log.trace.into(),
                Level::Info => self.config.log.info.into(),
                Level::Warn => self.config.log.warn.into(),
                Level::Error => self.config.log.error.into(),
                Level::Debug => self.config.log.debug.into(),
            };
            Text::styled(s, style)
        });
        let mut all_text = Text::default();
        for t in texts {
            all_text.extend(t);
        }
        let lines = all_text.lines.len();
        // let all = self.receiver.messages().iter().join("\n");
        let para = ScrollParagraph::new(all_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Log")
                .border_style(self.config.log.border_color.into()),
        );

        let mut state = self.state.take();
        if self.locked.get() {
            state.scroll_bottom();
        }
        let mut scrolls = self.scrolls.take();
        while let Some(ScrollEvent {
            direction,
            point: (x, y),
        }) = scrolls.pop_front()
        {
            let mouse_rect = Rect::new(x, y, 1, 1);
            if area.intersects(mouse_rect) {
                match direction {
                    ScrollDirection::Up => state.up(),
                    ScrollDirection::Down => state.down(),
                }
            }
        }
        f.render_stateful_widget(para, area, &mut state);
        // HACK: Check if scrolled to the bottom by re-calculating the expected offset of
        // ParagraphState.
        let len = (lines as u16).saturating_sub(area.height - 1);
        if len.saturating_sub(1) == state.offset_top {
            self.locked.set(true);
        }
        self.state.set(state);

        Ok(())
    }
}
