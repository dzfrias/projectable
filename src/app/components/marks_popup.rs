use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use crossterm::event::Event;
use easy_switch::switch;
use itertools::Itertools;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
use tui::{
    backend::Backend,
    layout::Rect,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

pub struct MarksPopup {
    // Must be an Rc<RefCell> so marks can be updated in filetree when changed
    marks: Rc<RefCell<Vec<PathBuf>>>,
    queue: Queue,
    open: bool,
    config: Rc<Config>,
    state: Cell<ListState>,
    root: PathBuf,
}

impl Default for MarksPopup {
    fn default() -> Self {
        Self::new(
            Default::default(),
            Queue::new(),
            Rc::new(Config::default()),
            ".".into(),
        )
    }
}

impl MarksPopup {
    pub fn new(
        marks: Rc<RefCell<Vec<PathBuf>>>,
        queue: Queue,
        config: Rc<Config>,
        root: PathBuf,
    ) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            marks,
            queue,
            config,
            root,
            state: state.into(),
            open: false,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.state.get_mut().select(Some(0));
        self.open = false;
    }

    pub fn add_mark(&mut self, path: PathBuf) {
        // Not a HashSet so it can be well-ordered
        if self.marks.borrow().contains(&path) {
            return;
        }
        self.marks.borrow_mut().push(path);
    }

    fn delete_selected(&mut self) {
        self.marks.borrow_mut().remove(self.selected());
        if let Some(selected) = self.state.get_mut().selected() {
            if selected >= self.marks.borrow().len() {
                self.select_first();
            }
        } else {
            self.select_first();
        }
    }

    fn selected(&self) -> usize {
        let state = self.state.take();
        let selected = state.selected().expect("should have something selected");
        self.state.set(state);
        selected
    }

    fn select_next(&mut self) {
        let current = self.selected();
        if self.marks.borrow().is_empty() || current == self.marks.borrow().len() - 1 {
            return;
        }
        self.state.get_mut().select(Some(current + 1));
    }

    fn select_prev(&mut self) {
        let current = self.selected();
        if current == 0 {
            return;
        }
        self.state.get_mut().select(Some(current - 1));
    }

    fn select_first(&mut self) {
        self.state.get_mut().select(Some(0));
    }

    fn select_last(&mut self) {
        if self.marks.borrow().is_empty() {
            return;
        }
        self.state
            .get_mut()
            .select(Some(self.marks.borrow().len() - 1));
    }
}

impl Drawable for MarksPopup {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        let marks = self.marks.borrow();
        let marks = marks
            .iter()
            .map(|mark| {
                ListItem::new(if self.config.marks.relative {
                    mark.strip_prefix(&self.root)
                        .expect("should start with root")
                        .as_os_str()
                        .to_string_lossy()
                } else {
                    mark.as_os_str().to_string_lossy()
                })
                .style(self.config.marks.mark_style.into())
            })
            .collect_vec();
        let list = List::new(marks)
            .highlight_style(self.config.selected.into())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.config.popup_border_style.into())
                    .title("Marks"),
            );
        let area = ui::centered_rect_absolute(50, 15, area);
        f.render_widget(Clear, area);
        let mut state = self.state.take();
        f.render_stateful_widget(list, area, &mut state);
        self.state.set(state);

        Ok(())
    }
}

impl Component for MarksPopup {
    fn visible(&self) -> bool {
        self.open
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        if let ExternalEvent::Crossterm(Event::Key(key)) = ev {
            switch! { key;
                self.config.quit => self.close(),
                self.config.down => self.select_next(),
                self.config.up => self.select_prev(),
                self.config.all_up => self.select_first(),
                self.config.all_down => self.select_last(),
                self.config.open => {
                    let selected = {
                        let marks = self.marks.borrow();
                        let selected = marks.get(self.selected()).cloned();
                        selected
                    };
                    // Will be `None` if there are no marks
                    if let Some(selected) = selected {
                        self.queue.add(AppEvent::GotoFile(selected));
                        self.close();
                    }
                },
                self.config.marks.delete => {
                    let selected = {
                        let marks = self.marks.borrow();
                        let selected = marks.get(self.selected()).cloned();
                        selected
                    };
                    // Will be `None` if there are no marks
                    if let Some(selected) = selected {
                        self.queue.add(AppEvent::DeleteMark(selected));
                        self.delete_selected();
                    }
                },
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::components::testing::*;
    use test_log::test;

    fn test_popup() -> MarksPopup {
        let mut popup = MarksPopup::new(
            Rc::new(RefCell::new(vec![".".into(), "/".into()])),
            Queue::new(),
            Rc::new(Config::default()),
            ".".into(),
        );
        popup.open();
        popup
    }

    #[test]
    fn new_selects_fist_item() {
        let popup = test_popup();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn adding_marks_is_unique() {
        let mut popup = test_popup();
        assert_eq!(2, popup.marks.borrow().len());
        popup.add_mark(".".into());
        assert_eq!(2, popup.marks.borrow().len());
    }

    #[test]
    fn can_delete_marks() {
        let mut popup = test_popup();
        popup.delete_selected();
        let marks = popup.marks.borrow();
        assert_eq!(PathBuf::from("/"), marks[0])
    }

    #[test]
    fn deleting_last_mark_wraps_selected_to_top() {
        let mut popup = test_popup();
        popup.state.get_mut().select(Some(1));
        popup.delete_selected();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn does_not_panic_with_zero_marks() {
        let mut popup = test_popup();
        popup.marks = Default::default();
        let events = input_events!(
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('g'),
            KeyCode::Char('G'); KeyModifiers::SHIFT,
            KeyCode::Enter,
            KeyCode::Char('d')
        );
        for event in events {
            assert!(popup.handle_event(&event).is_ok());
        }
    }

    #[test]
    fn can_select_first() {
        let mut popup = test_popup();
        popup.state.get_mut().select(Some(1));
        popup.select_first();
        assert_eq!(0, popup.selected());
    }

    #[test]
    fn can_select_last() {
        let mut popup = test_popup();
        popup.marks = Rc::new(RefCell::new(vec![
            ".".into(),
            "/".into(),
            "/test.txt".into(),
        ]));
        popup.select_last();
        assert_eq!(2, popup.selected())
    }

    #[test]
    fn visible_with_work_and_vice_versa() {
        let mut popup = test_popup();
        assert!(popup.visible());
        popup.close();
        assert!(!popup.visible());
    }

    #[test]
    fn resets_selected_when_closed() {
        let mut popup = test_popup();
        popup.select_next();
        popup.close();
        assert_eq!(0, popup.selected());
    }
}
