use easy_cast::CastFloat;
use tui::{
    buffer::Buffer,
    layout::{Alignment, Margin, Rect},
    style::Style,
    symbols::{block::FULL, line::DOUBLE_VERTICAL},
    text::Text,
    widgets::{Block, Paragraph, StatefulWidget, Widget},
};

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct ParagraphState {
    pub offset_top: u16,
}

impl ParagraphState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn down(&mut self) {
        self.offset_top = self.offset_top.saturating_add(1);
    }

    pub fn down_by(&mut self, amount: u16) {
        for _ in 0..amount {
            self.down();
        }
    }

    pub fn up_by(&mut self, amount: u16) {
        for _ in 0..amount {
            self.up();
        }
    }

    pub fn up(&mut self) {
        if self.offset_top == 0 {
            return;
        }
        self.offset_top -= 1;
    }

    pub fn reset(&mut self) {
        self.offset_top = 0;
    }
}

#[derive(Debug, Clone)]
pub struct ScrollParagraph<'a> {
    text: Text<'a>,
    block: Option<Block<'a>>,
    style: Style,
    alignment: Alignment,
    bar_style: Style,
    unreached_bar_style: Style,
    bar_margins: u8,
}

impl<'a> ScrollParagraph<'a> {
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            block: None,
            style: Style::default(),
            bar_style: Style::default(),
            alignment: Alignment::Left,
            unreached_bar_style: Style::default(),
            bar_margins: 1,
        }
    }

    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    #[must_use]
    pub fn bar_style(mut self, style: Style) -> Self {
        self.bar_style = style;
        self
    }

    #[must_use]
    pub fn unreached_bar_style(mut self, style: Style) -> Self {
        self.unreached_bar_style = style;
        self
    }

    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    #[must_use]
    pub fn bar_margins(mut self, bar_margins: u8) -> Self {
        self.bar_margins = bar_margins;
        self
    }
}

impl<'a> StatefulWidget for ScrollParagraph<'a> {
    type State = ParagraphState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Thanks to https://github.com/extrawurst/gitui/blob/v0.22.1/src/ui/scrollbar.rs for a lot
        // of the rendering of the scrollbar

        let area = self.block.take().map_or(area, |b| {
            let inner_area = b.inner(area);
            b.render(area, buf);
            inner_area
        });

        let len = self.text.lines.len() as u16;
        state.offset_top = state.offset_top.min(len - 1);

        buf.set_style(area, self.style);
        let paragraph = Paragraph::new(self.text)
            .scroll((state.offset_top, 0))
            .alignment(self.alignment);
        paragraph.render(area, buf);

        if len == 0 || area.width <= 2 {
            return;
        }

        let right = area.right();
        if right <= area.left() {
            return;
        };

        let (bar_top, bar_height) = {
            let scrollbar_area = area.inner(&Margin {
                horizontal: 0,
                // Set top/bottom padding
                vertical: self.bar_margins as u16,
            });

            (scrollbar_area.top(), scrollbar_area.height)
        };

        for y in bar_top..(bar_top + bar_height) {
            buf.set_string(right, y, DOUBLE_VERTICAL, self.unreached_bar_style);
        }

        let progress = state.offset_top as f32 / len as f32;
        let progress = if progress > 1.0 { 1.0 } else { progress };
        let pos: u16 = (bar_height as f32 * progress).cast_nearest();
        let pos = pos.saturating_sub(1);

        let mut divisions: u16 = (bar_height as f32 / len as f32).cast_nearest();
        if divisions == bar_height {
            // Overflow with one-line texts
            divisions -= 1;
        }
        for y in 0..=divisions {
            buf.set_string(right, bar_top + pos + y, FULL, self.bar_style);
        }
    }
}
