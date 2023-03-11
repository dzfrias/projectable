use anyhow::Result;
use projectable::{app::App, ui};
use std::{
    io::{self, Stdout},
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify::RecursiveMode;
use tui::{backend::CrosstermBackend, Terminal};

fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new(".")?;
    run_app(&mut terminal, &mut app)?;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    // TODO: Turn into module
    let (tx, rx) = mpsc::channel();
    let mut bouncer =
        notify_debouncer_mini::new_debouncer(Duration::from_secs(1), None, tx).unwrap();
    bouncer
        .watcher()
        .watch(&app.path, RecursiveMode::Recursive)?;
    std::mem::forget(bouncer);

    // TODO: Make proper channel for sending and receiving events
    // see https://github.com/extrawurst/gitui/blob/63f230f0d1de5b06b325b11924eb41f6120b30da/src/main.rs#L182
    let (event_send, event_recv) = mpsc::channel();

    thread::spawn(move || loop {
        let ev = rx.recv().unwrap();
        if let Ok(ev) = ev {
            if !ev.is_empty() {
                event_send.send(()).unwrap();
            }
        };
    });

    loop {
        if event_recv.try_recv().is_ok() {
            app.tree.refresh().unwrap();
        }
        terminal.draw(|f| ui::ui(f, app))?;

        // TODO: Turn into module that sends an event and uses event channel
        if event::poll(Duration::from_millis(300))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char(c) => app.handle_key(c),
                    KeyCode::Up => app.on_up(),
                    KeyCode::Down => app.on_down(),
                    KeyCode::Left => app.on_left(),
                    KeyCode::Right => app.on_right(),
                    KeyCode::Enter => app.on_enter(),
                    _ => {}
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
