use super::ExternalEvent;
use anyhow::Result;
use crossbeam_channel::{unbounded, Sender};
use notify::{recommended_watcher, Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, thread, time::Duration};

/// Watch for changes to the filesystem at `path`, sending results to `event_sender`
pub fn fs_watch(
    path: &Path,
    event_sender: Sender<ExternalEvent>,
    refresh_time: u64,
) -> Result<RecommendedWatcher> {
    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx)?;
    watcher.configure(Config::default().with_poll_interval(Duration::from_millis(refresh_time)))?;
    watcher.watch(path, RecursiveMode::Recursive)?;
    thread::spawn(move || {
        for res in rx {
            match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) | EventKind::Remove(_) => {
                        event_sender.send(ExternalEvent::RefreshFiletree).unwrap()
                    }
                    _ => {}
                },
                Err(e) => println!("watch error: {:?}", e),
            }
        }
    });

    Ok(watcher)
}
