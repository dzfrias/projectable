use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use crossbeam_channel::unbounded;
use log::{error, warn, LevelFilter};
use projectable::{
    app::{component::Drawable, App, TerminalEvent},
    config::{self, Config, GlobList, Merge},
    external_event,
    marks::{self, Marks},
};
use std::{
    cell::RefCell,
    env, fs,
    io::{self, Stdout},
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    dir: Option<PathBuf>,

    #[arg(long, help = "Debug mode")]
    debug: bool,
    #[arg(short, long, help = "Print config location")]
    config: bool,
    #[arg(long, help = "Print marks file location")]
    marks_file: bool,
    #[arg(long, help = "Create a default config")]
    write_config: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.config {
        println!(
            "{}",
            config::get_config_home()
                .context("could not find config home")?
                .display()
        );
        return Ok(());
    } else if args.marks_file {
        println!(
            "{}",
            marks::get_marks_file()
                .context("could not find config home")?
                .display()
        );
        return Ok(());
    } else if args.write_config {
        let config_file = config::get_config_home()
            .context("could not find config home")?
            .join("config.toml");
        let new_config = Config::default();

        fs::write(&config_file, toml::to_string(&new_config)?)?;
        println!("Wrote to config file at {}!", config_file.display());
        return Ok(());
    }

    // Set up raw mode, etc.
    setup()?;

    // Restore terminal
    defer_on_success! {
        shut_down();
    }

    let config = Rc::new(get_config()?);

    // Logging setup
    #[cfg(debug_assertions)]
    tui_logger::init_logger(LevelFilter::Debug).unwrap();
    #[cfg(not(debug_assertions))]
    if !args.debug {
        tui_logger::init_logger(config.log.log_level).unwrap();
    } else {
        tui_logger::init_logger(LevelFilter::Debug).unwrap();
    }
    tui_logger::set_default_level(LevelFilter::Trace);

    // Check keybind conflicts
    let conflicts = config.check_conflicts();
    for conflict in conflicts {
        warn!("{conflict}");
    }

    // Create tui terminal and app
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let root = find_project_root(&config.project_roots)?.ok_or(anyhow!("not in a project!"))?;
    let dir = args.dir.map_or(env::current_dir()?, |dir| root.join(dir));
    let marks = Rc::new(RefCell::new(Marks::from_marks_file(&root)?));
    let mut app = App::new(root, dir, Rc::clone(&config), Rc::clone(&marks))
        .context("failed to create app")?;

    // Begin app event loop
    run_app(&mut terminal, &mut app, Rc::clone(&config), marks)?;

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

/// Get the project root. This function searches for a `.git` directory. Errors if the current
/// directory is invalid, and returns `None` if there was no root found.
fn find_project_root(globs: &GlobList) -> Result<Option<PathBuf>> {
    let start = env::current_dir()?;
    Ok(start.ancestors().find_map(|path| {
        fs::read_dir(path)
            .ok()?
            .filter_map(|entry| entry.ok())
            .any(|entry| globs.is_match(entry.path()))
            .then(|| path.to_path_buf())
    }))
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
    marks: Rc<RefCell<Marks>>,
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
            },
            Err(err) => {
                error!("{err:#}");
            }
            Ok(None) => {}
        }
        terminal.draw(|f| app.draw(f, f.size()).unwrap())?;

        if app.should_quit() {
            marks.borrow_mut().write()?;
            return Ok(());
        }
    }
}

fn setup() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    panic::set_hook(Box::new(|info| {
        shut_down();
        let meta = human_panic::metadata!();
        let file_path = human_panic::handle_dump(&meta, info);
        human_panic::print_msg(file_path, &meta)
            .expect("human-panic: printing error message to console failed");
    }));

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
