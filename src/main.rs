use anyhow::{bail, Result};
use crossbeam_channel::{unbounded, TryRecvError};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    event,
};
use std::{
    env,
    io::{self, Stdout},
    process::Command,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
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
    let (event_send, event_recv) = unbounded();
    event::fs_watch(app.path(), event_send.clone())?;
    event::crossterm_watch(event_send);

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
        match event {
            TerminalEvent::OpenFile(path) => {
                let editor = env::var("EDITOR").unwrap_or("vi".to_owned());
                Command::new(editor).arg(path).status()?;
                let mut stdout = io::stdout();
                execute!(stdout, EnterAlternateScreen)?;
                terminal.clear()?;
            }
            TerminalEvent::Nothing => {}
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}
