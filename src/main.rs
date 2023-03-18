use anyhow::{bail, Result};
use crossbeam_channel::{unbounded, TryRecvError};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    external_event,
};
use std::{
    env,
    io::{self, Stdout},
    panic,
    process::Command,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use scopeguard::defer_on_success;
use tui::{backend::CrosstermBackend, Terminal};

fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Restore terminal
    defer_on_success! {
        shut_down();
    }

    panic::set_hook(Box::new(|info| {
        shut_down();
        eprintln!("panicked: {info}");
        eprintln!("please report this issue on GitHub");
    }));

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut app = App::new(".")?;
    run_app(&mut terminal, &mut app)?;

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    // Set up event channel
    let (event_send, event_recv) = unbounded();
    external_event::fs_watch(app.path(), event_send.clone())?;
    external_event::crossterm_watch(event_send);

    loop {
        match event_recv.try_recv() {
            Ok(event) => {
                app.handle_event(&event)?;
            }
            Err(TryRecvError::Empty) => {}
            Err(err) => bail!(err),
        }

        terminal.draw(|f| app.draw(f, f.size()).unwrap())?;
        let event = app.update()?;
        if let Some(event) = event {
            match event {
                TerminalEvent::OpenFile(path) => {
                    let editor = env::var("EDITOR").unwrap_or("vi".to_owned());
                    Command::new(editor).arg(path).status()?;
                    let mut stdout = io::stdout();
                    execute!(stdout, EnterAlternateScreen)?;
                    terminal.clear()?;
                }
            }
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}

fn shut_down() {
    let mut stdout = io::stdout();
    let leave_screen = execute!(stdout, LeaveAlternateScreen);
    if let Err(err) = leave_screen {
        eprintln!("could not leave screen:\n{err}");
    }
    let disable_raw_mode = disable_raw_mode();
    if let Err(err) = disable_raw_mode {
        eprintln!("could not disable raw mode:\n{err}");
    }
    let disable_mouse_capture = execute!(stdout, DisableMouseCapture);
    if let Err(err) = disable_mouse_capture {
        eprintln!("could not disable mouse capture:\n{err}");
    }
}
