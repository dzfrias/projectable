use tui_tree_widget::Tree;

use crate::app::App;
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Span,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
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
    draw_confirm_popup(f, app);
}

fn draw_file_tree(f: &mut Frame<impl Backend>, app: &mut App, area: Rect) {
    let app_tree = app.tree_mut();
    let items = Tree::new(app_tree.files.items().clone())
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
    f.render_stateful_widget(items, area, &mut app_tree.state);
}

fn draw_confirm_popup(f: &mut Frame<impl Backend>, app: &mut App) {
    if !app.pending.has_work() {
        return;
    }
    let items = [ListItem::new("Confirm"), ListItem::new("Deny")];
    let list =
        List::new(items).highlight_style(Style::default().fg(Color::Black).bg(Color::LightGreen));
    let area = centered_rect(30, 20, f.size());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Percentage(50)].as_ref())
        .horizontal_margin(2)
        .vertical_margin(2)
        .split(area);
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default()
            .title("Confirm")
            .borders(Borders::ALL)
            .title_alignment(Alignment::Center),
        area,
    );
    f.render_widget(
        Paragraph::new("Are you sure you want to delete this file/directory?")
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        layout[0],
    );
    f.render_stateful_widget(list, layout[1], &mut app.pending.state);
}

/// Center a `Rect` with a height and width as a percentage of `r`
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}
