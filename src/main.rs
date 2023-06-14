use anyhow::{anyhow, bail, Context, Result};
use crossbeam_channel::unbounded;
use log::{error, warn, LevelFilter};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    config::{self, Config, Merge},
    external_event,
    marks::{self, Marks},
};
use std::{
    cell::RefCell,
    collections::hash_map::Entry,
    env,
    fs::{self, File},
    io::{self, Stdout, Write},
    panic,
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use scopeguard::{defer, defer_on_success};
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
        let meta = human_panic::metadata!();
        let file_path = human_panic::handle_dump(&meta, info);
        human_panic::print_msg(file_path, &meta)
            .expect("human-panic: printing error message to console failed");
    }));

    let config = Rc::new(get_config()?);

    #[cfg(debug_assertions)]
    tui_logger::init_logger(LevelFilter::Debug).unwrap();
    #[cfg(not(debug_assertions))]
    tui_logger::init_logger(config.log.log_level).unwrap();

    tui_logger::set_default_level(LevelFilter::Trace);
    let conflicts = config.check_conflicts();
    for conflict in conflicts {
        warn!("{conflict}");
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let root = find_project_root()?.ok_or(anyhow!("not in a project!"))?;
    let mut all_marks = get_marks()?;
    let project_marks = all_marks.marks.remove(&root).unwrap_or_default();
    let mut app = App::new(
        root,
        env::current_dir()?,
        Rc::clone(&config),
        Rc::new(RefCell::new(project_marks)),
    )
    .context("failed to create app")?;
    run_app(&mut terminal, &mut app, Rc::clone(&config))?;

    Ok(())
}

fn get_config() -> Result<Config> {
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
    if let Some(local_config) = find_local_config()? {
        let contents = fs::read_to_string(local_config)?;
        let local_config = toml::from_str(&contents)?;
        config.merge(local_config);
    }

    Ok(config)
}

fn get_marks() -> Result<Marks> {
    marks::get_marks_file()
        .map(|path| -> Result<Marks> {
            if !path.exists() {
                fs::create_dir_all(path.parent().expect("data dir should have parent"))?;
                let mut file = File::create(path)?;
                file.write_all(b"{}")?;
                return Ok(Marks::default());
            }
            let contents = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&contents)?)
        })
        .unwrap_or(Ok(Marks::default()))
}

/// Get the project root. This function searches for a `.git` directory. Errors if the current
/// directory is invalid, and returns `None` if there was no root found.
fn find_project_root() -> Result<Option<PathBuf>> {
    let start = env::current_dir()?;
    Ok(start
        .ancestors()
        .find_map(|path| path.join(".git").is_dir().then(|| path.to_path_buf())))
}

/// Gets the local configuration file. Errors if the current directory is invalid, and returns
/// `None` if there was no `.projectable.toml` found.
fn find_local_config() -> Result<Option<PathBuf>> {
    let start = env::current_dir()?;
    Ok(start.ancestors().find_map(|path| {
        let new_path = path.join(".projectable.toml");
        if new_path.exists() {
            Some(new_path)
        } else {
            None
        }
    }))
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

    let stop = Arc::new(AtomicBool::new(false));
    let mut input_handle = external_event::crossterm_watch(event_send.clone(), Arc::clone(&stop));

    // When set to true, will stop any running child processes of projectable
    let thread_stop = Arc::new(AtomicBool::new(false));

    let mut first_run = true;
    loop {
        if first_run {
            first_run = false;
        } else {
            match event_recv.recv() {
                Ok(event) => {
                    if let Err(err) = app.handle_event(&event) {
                        error!("{err:#}");
                    }
                }
                Err(err) => bail!(err),
            }
        }

        match app.update() {
            Ok(Some(event)) => match event {
                TerminalEvent::OpenFile(path) => {
                    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
                    disable_raw_mode()?;
                    defer! {
                        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture).expect("error setting up screen");
                        enable_raw_mode().expect("error enabling raw mode");
                        io::stdout().execute(EnterAlternateScreen).expect("error entering alternate screen");
                        terminal.clear().expect("error clearing terminal");
                    }
                    let editor = env::var("EDITOR").unwrap_or("vi".to_owned());
                    // Join the input receiving thread by setting `stop_flag` to true
                    stop.store(true, Ordering::Release);
                    input_handle.join().expect("error joining thread");
                    Command::new(editor).arg(path).status()?;
                    // Resume input receiving thread again
                    stop.store(false, Ordering::Release);
                    input_handle =
                        external_event::crossterm_watch(event_send.clone(), Arc::clone(&stop));
                }
                TerminalEvent::WriteMark(path) => match get_marks() {
                    Ok(mut marks) => {
                        match marks.marks.entry(app.path().to_path_buf()) {
                            Entry::Vacant(entry) => drop(entry.insert(vec![path])),
                            Entry::Occupied(mut entry) => {
                                if !entry.get().contains(&path) {
                                    entry.get_mut().push(path)
                                }
                            }
                        }
                        if let Err(err) = write_marks(&marks) {
                            error!("{err}")
                        }
                    }
                    Err(err) => error!("{err}"),
                },
                TerminalEvent::RunCommandThreaded(expr) => {
                    thread_stop.store(false, Ordering::Release);
                    external_event::run_cmd(
                        expr,
                        event_send.clone(),
                        Duration::from_millis(300),
                        thread_stop.clone(),
                    )?
                }
                TerminalEvent::RunCommand(expr) => {
                    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
                    disable_raw_mode()?;
                    defer! {
                        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture).expect("error setting up screen");
                        enable_raw_mode().expect("error enabling raw mode");
                        io::stdout().execute(EnterAlternateScreen).expect("error entering alternate screen");
                        terminal.clear().expect("error clearing terminal");
                    }
                    // Join the input receiving thread by setting `stop_flag` to true
                    stop.store(true, Ordering::Release);
                    input_handle.join().expect("error joining thread");
                    expr.start()?.wait()?;
                    // Resume input receiving thread again
                    stop.store(false, Ordering::Release);
                    input_handle =
                        external_event::crossterm_watch(event_send.clone(), Arc::clone(&stop));
                }
                TerminalEvent::StopAllCommands => thread_stop.store(true, Ordering::Release),
                TerminalEvent::DeleteMark(path) => match get_marks() {
                    Ok(mut marks) => {
                        match marks.marks.entry(app.path().to_path_buf()) {
                            Entry::Vacant(_) => {
                                error!("trying to delete mark that doesn't exist")
                            }
                            Entry::Occupied(mut entry) => {
                                let position = entry.get().iter().position(|p| p == &path);
                                if let Some(position) = position {
                                    entry.get_mut().remove(position);
                                } else {
                                    error!("trying to delete mark that doesn't exist")
                                }
                            }
                        }
                        if let Err(err) = write_marks(&marks) {
                            error!("{err}")
                        }
                    }
                    Err(err) => error!("{err}"),
                },
            },
            Err(err) => {
                error!("{err:#}");
            }
            Ok(None) => {}
        }
        terminal.draw(|f| app.draw(f, f.size()).unwrap())?;

        if app.should_quit() {
            return Ok(());
        }
    }
}

fn write_marks(marks: &Marks) -> Result<()> {
    let json = serde_json::to_string(&marks)?;
    fs::write(
        marks::get_marks_file().expect("should not error here, would have errored earlier"),
        json,
    )?;
    Ok(())
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
