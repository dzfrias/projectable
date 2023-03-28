use anyhow::{anyhow, bail, Result};
use crossbeam_channel::unbounded;
use log::{error, warn, LevelFilter};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    config::{self, Config, Merge},
    external_event,
};
use std::{
    env, fs,
    io::{self, Stdout},
    panic,
    path::PathBuf,
    process::Command,
    rc::Rc,
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
        let meta = human_panic::metadata!();
        let file_path = human_panic::handle_dump(&meta, info);
        human_panic::print_msg(file_path, &meta)
            .expect("human-panic: printing error message to console failed");
    }));

    let mut config = config::get_config_home()
        .map(|path| -> Result<Option<Config>> {
            if !path.join("config.toml").exists() {
                return Ok(None);
            }
            let contents = fs::read_to_string(path.join("config.toml"))?;
            Ok(Some(toml::from_str::<Config>(&contents)?))
        })
        .unwrap_or(Ok(Some(Config::default())))?
        .unwrap_or(Config::default());
    if let Some(local_config) = find_local_config() {
        let contents = fs::read_to_string(local_config)?;
        let local_config = toml::from_str(&contents)?;
        config.merge(local_config);
    }
    let config = Rc::new(config);
    let conflicts = config.check_conflicts();
    for conflict in conflicts {
        warn!("{conflict}");
    }
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let root = find_project_root().ok_or(anyhow!("not in a project!"))?;
    let mut app = App::new(root, env::current_dir()?, Rc::clone(&config))?;
    run_app(&mut terminal, &mut app, Rc::clone(&config))?;

    Ok(())
}

fn find_project_root() -> Option<PathBuf> {
    let start = fs::canonicalize(".").expect("should be valid path");
    start
        .ancestors()
        .find_map(|path| path.join(".git").is_dir().then(|| path.to_path_buf()))
}

fn find_local_config() -> Option<PathBuf> {
    let start = fs::canonicalize(".").expect("should be valid path");
    start.ancestors().find_map(|path| {
        let new_path = path.join(".projectable.toml");
        if new_path.exists() {
            Some(new_path)
        } else {
            None
        }
    })
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    config: Rc<Config>,
) -> Result<()> {
    // Set up event channel
    let (event_send, event_recv) = unbounded();
    let _watcher =
        external_event::fs_watch(app.path(), event_send.clone(), config.filetree.refresh_time)?;
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
