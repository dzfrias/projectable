use anyhow::{bail, Result};
use projectable::{
    app::App,
    event::{self, EventType},
    ui,
};
use std::{
    io::{self, Stdout},
    sync::mpsc,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
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
    // Set up event channel
    let (event_send, event_recv) = mpsc::channel();
    event::fs_watch(&app.path, event_send.clone())?;
    event::crossterm_watch(event_send);

    loop {
        match event_recv.try_recv() {
            Ok(event) => match event {
                EventType::RefreshFiletree => app.tree.refresh()?,
                EventType::Crossterm(ev) => {
                    if let Event::Key(key) = ev {
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
                EventType::Error(err) => bail!(err),
            },
            Err(mpsc::TryRecvError::Empty) => {}
            Err(err) => bail!(err),
        }

        terminal.draw(|f| ui::ui(f, app))?;

        if app.should_quit {
            return Ok(());
        }
    }
}
