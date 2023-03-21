use anyhow::{anyhow, bail, Result};
use crossbeam_channel::unbounded;
use log::{error, LevelFilter};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    external_event,
};
use std::{
    env, fs,
    io::{self, Stdout},
    panic,
    path::PathBuf,
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
    tui_logger::init_logger(LevelFilter::Info).unwrap();
    tui_logger::set_default_level(LevelFilter::Trace);

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
    let root = find_project_root().ok_or(anyhow!("not in a project!"))?;
    let mut app = App::new(root, env::current_dir()?)?;
    run_app(&mut terminal, &mut app)?;

    Ok(())
}

fn find_project_root() -> Option<PathBuf> {
    let start = fs::canonicalize(".").expect("should be valid path");
    start
        .ancestors()
        .find(|path| path.join(".git").is_dir())
        .map(|path| path.to_path_buf())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    // Set up event channel
    let (event_send, event_recv) = unbounded();
    external_event::fs_watch(app.path(), event_send.clone())?;
    external_event::crossterm_watch(event_send);

    let mut first_run = true;
    loop {
        if first_run {
            first_run = false;
        } else {
            match event_recv.recv() {
                Ok(event) => {
                    if let Err(err) = app.handle_event(&event) {
                        error!(" {}", err);
                    }
                }
                Err(err) => bail!(err),
            }
        }

        match app.update() {
            Ok(Some(event)) => match event {
                TerminalEvent::OpenFile(path) => {
                    let editor = env::var("EDITOR").unwrap_or("vi".to_owned());
                    Command::new(editor).arg(path).status()?;
                    let mut stdout = io::stdout();
                    execute!(stdout, EnterAlternateScreen)?;
                    terminal.clear()?;
                }
            },
            Err(err) => {
                error!(" {}", err);
            }
            Ok(None) => {}
        }
        terminal.draw(|f| app.draw(f, f.size()).unwrap())?;

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
