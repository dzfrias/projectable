use tui_tree_widget::Tree;

use crate::app::App;
use tui::backend::Backend;
use tui::text::Span;
use tui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn ui(f: &mut Frame<impl Backend>, app: &mut App) {
    let main_layout = Layout::default()
        .direction(Direction::Horizontal)
        .horizontal_margin(1)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(f.size());
    let left_hand_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)].as_ref())
        .split(main_layout[0]);

    let text = vec![
        Span::raw("hi").into(),
        Span::styled("Second line", Style::default().fg(Color::Red)).into(),
    ];
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    let block = Block::default().title("Block").borders(Borders::ALL);

    draw_file_tree(f, app, left_hand_layout[0]);
    f.render_widget(block, left_hand_layout[1]);
    f.render_widget(p, main_layout[1]);
}

fn draw_file_tree(f: &mut Frame<impl Backend>, app: &mut App, area: Rect) {
    let app_tree = app.tree_mut();
    let items = Tree::new(app_tree.files.items().clone())
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
    f.render_stateful_widget(items, area, &mut app_tree.state);
}
