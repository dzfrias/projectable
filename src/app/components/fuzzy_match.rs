use crate::{
    app::component::{Component, Drawable},
    config::Config,
    external_event::ExternalEvent,
    queue::{AppEvent, Queue},
    ui,
};
use anyhow::Result;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher as Matcher};
use itertools::Itertools;
use std::{cell::Cell, rc::Rc};
use tui::{
    backend::Backend,
    layout::{Constraint, Corner, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};
use tui_textarea::{Input, Key, TextArea};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FuzzyOperation {
    OpenFile,
    None,
}

pub struct FuzzyMatcher {
    input: Vec<String>,
    area: TextArea<'static>,
    operation: FuzzyOperation,
    state: Cell<ListState>,
    config: Rc<Config>,
    queue: Queue,
}

impl FuzzyMatcher {
    pub fn new(queue: Queue) -> Self {
        let mut textarea = TextArea::default();
        textarea.set_block(Block::default().borders(Borders::ALL));
        Self {
            input: Vec::new(),
            area: textarea,
            operation: FuzzyOperation::None,
            state: ListState::default().into(),
            config: Rc::new(Config::default()),
            queue,
        }
    }

    pub fn new_with_config(queue: Queue, config: Rc<Config>) -> Self {
        Self {
            config,
            ..Self::new(queue)
        }
    }

    pub fn start(&mut self, items: Vec<String>, operation: FuzzyOperation) {
        self.operation = operation;
        self.input = items;
        self.state.get_mut().select(Some(0));
    }

    pub fn open_path(&mut self, items: Vec<String>) {
        self.start(items, FuzzyOperation::OpenFile);
    }

    pub fn compute_best_matches(&self) -> Vec<(&str, Vec<usize>)> {
        let match_against = &self.area.lines()[0];
        let matcher = SkimMatcherV2::default();
        self.input
            .iter()
            .filter_map(|option| {
                matcher
                    .fuzzy_indices(option, match_against)
                    .map(|m| (option.as_str(), m))
            })
            .sorted_by(|a, b| a.1 .0.cmp(&b.1 .0))
            .map(|(option, (_, indices))| (option, indices))
            .rev()
            .collect()
    }

    pub fn reset(&mut self) {
        self.area = TextArea::default();
        self.area.set_block(Block::default().borders(Borders::ALL));
        self.operation = FuzzyOperation::None;
        self.input = Vec::new();
        self.state = ListState::default().into();
    }

    pub fn submit(&mut self) {
        let Some(sel) = self.selected() else { return; };
        let matches = self.compute_best_matches();
        let Some(selected) = matches.get(sel) else { return; };
        match self.operation {
            FuzzyOperation::OpenFile => self
                .queue
                .add(AppEvent::GotoFile(selected.0.to_owned().into())),
            FuzzyOperation::None => panic!("should not submit with no operation"),
        }
        self.reset();
    }

    pub fn select_next(&mut self) {
        let old = self.state.get_mut().selected().unwrap_or_default();
        self.state
            .get_mut()
            .select(Some(Ord::min(old + 1, self.input.len() - 1)));
    }

    pub fn select_prev(&mut self) {
        let old = self.state.get_mut().selected().unwrap_or_default();
        self.state.get_mut().select(Some(old.saturating_sub(1)));
    }

    pub fn selected(&self) -> Option<usize> {
        let state = self.state.take();
        let selected = state.selected();
        self.state.set(state);
        selected
    }
}

impl Component for FuzzyMatcher {
    fn visible(&self) -> bool {
        self.operation != FuzzyOperation::None
    }

    fn handle_event(&mut self, ev: &ExternalEvent) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        if let ExternalEvent::Crossterm(ev) = ev {
            let input_event: Input = ev.clone().into();
            match input_event {
                Input {
                    key: Key::Esc,
                    ctrl: false,
                    alt: false,
                } => {
                    self.reset();
                    return Ok(());
                }
                Input {
                    key: Key::Enter,
                    ctrl: false,
                    alt: false,
                } => {
                    self.submit();
                    return Ok(());
                }
                Input {
                    key: Key::Char('n'),
                    alt: false,
                    ctrl: true,
                } => {
                    self.select_prev();
                }
                Input {
                    key: Key::Char('p'),
                    alt: false,
                    ctrl: true,
                } => {
                    self.select_next();
                }
                Input {
                    key: Key::Char('u'),
                    ctrl: true,
                    alt: false,
                } => {
                    self.area = TextArea::default();
                    self.area.set_block(Block::default().borders(Borders::ALL));
                }
                _ => {}
            }
            self.area.input(input_event);
        }

        Ok(())
    }
}

impl Drawable for FuzzyMatcher {
    fn draw<B: Backend>(&self, f: &mut Frame<B>, area: Rect) -> Result<()> {
        if !self.visible() {
            return Ok(());
        }

        let area = ui::centered_rect(40, 25, area);
        f.render_widget(Clear, area);
        let [options_area, prompt_area] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)].as_ref())
            .split(area)[..] else {
                unreachable!("should always have 2 sections");
            };
        let options = List::new(
            self.compute_best_matches()
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    ListItem::new(Spans::from(
                        item.0
                            .chars()
                            .enumerate()
                            .map(|(c_idx, c)| {
                                Span::styled(
                                    c.to_string(),
                                    if item.1.contains(&c_idx) {
                                        Style::default().fg(Color::Blue)
                                    } else if self.selected().is_some()
                                        && self.selected().unwrap() == index
                                    {
                                        Style::default().fg(Color::Black)
                                    } else {
                                        Style::default()
                                    },
                                )
                            })
                            .collect_vec(),
                    ))
                })
                .collect_vec(),
        )
        .block(Block::default().borders(Borders::ALL))
        .start_corner(Corner::BottomLeft)
        .highlight_style(self.config.selected.into());
        let mut state = self.state.take();
        f.render_widget(self.area.widget(), prompt_area);
        f.render_stateful_widget(options, options_area, &mut state);
        self.state.set(state);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_best_matches_gives_sorted_list_of_matches_with_indices() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        matcher.input = vec![
            "/root/test.txt".to_owned(),
            "/root/test2.txt".to_owned(),
            "/root/2tests.txt".to_owned(),
            "/root/test/testingthis32.txt".to_owned(),
            "none!".to_owned(),
        ];
        matcher.area.insert_str("t2");
        let matches = matcher.compute_best_matches();
        assert_eq!(
            vec![
                ("/root/2tests.txt", vec![4, 6]),
                ("/root/test2.txt", vec![9, 10]),
                ("/root/test/testingthis32.txt", vec![11, 23]),
            ],
            matches
        );
    }

    #[test]
    fn submit_resets_everything() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        matcher.operation = FuzzyOperation::OpenFile;
        matcher.input = vec!["test".to_owned(), "testing".to_owned()];
        matcher.state.get_mut().select(Some(1));
        matcher.submit();
        assert!(matcher.selected().is_none());
        assert!(matcher.input.is_empty());
        assert_eq!(FuzzyOperation::None, matcher.operation);
    }

    #[test]
    fn select_prev_saturates_at_zero() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        assert!(matcher.selected().is_none());
        matcher.select_prev();
        assert_eq!(0, matcher.selected().unwrap());
    }

    #[test]
    fn select_next_saturates_at_length_of_input() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        matcher.operation = FuzzyOperation::OpenFile;
        matcher.input = vec!["test".to_owned(), "testing".to_owned()];
        matcher.select_next();
        matcher.select_next();
        matcher.select_next();
        matcher.select_next();
        matcher.select_next();
        matcher.select_next();

        assert_eq!(1, matcher.selected().unwrap());
    }

    #[test]
    fn submit_does_not_panic_with_no_matching_items() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        matcher.operation = FuzzyOperation::OpenFile;
        matcher.input = vec!["item".to_owned()];
        matcher.area.insert_str("sdlkfjslkddf");
        matcher.submit();
    }

    #[test]
    fn submit_gets_selected_match() {
        let mut matcher = FuzzyMatcher::new(Queue::new());
        matcher.operation = FuzzyOperation::OpenFile;
        matcher.input = vec!["item".to_owned(), "item2".to_owned()];
        matcher.area.insert_str("it");
        assert_eq!(
            vec![("item2", vec![0, 1]), ("item", vec![0, 1])],
            matcher.compute_best_matches()
        );
        matcher.select_prev();
        matcher.submit();
        assert!(matcher
            .queue
            .contains(&AppEvent::GotoFile("item2".to_owned().into())));
    }
}
