use crossbeam_channel::{unbounded, Sender};
use std::{path::Path, thread, time::Duration};

use super::ExternalEvent;
use anyhow::Result;
use notify::RecursiveMode;

/// Watch for changes to the filesystem at `path`, sending results to `event_sender`
pub fn fs_watch(path: &Path, event_sender: Sender<ExternalEvent>) -> Result<()> {
    const REFRESH_TIME_SECS: u64 = 1;

    let (tx, rx) = unbounded();
    let mut bouncer =
        notify_debouncer_mini::new_debouncer(Duration::from_secs(REFRESH_TIME_SECS), None, tx)?;
    bouncer.watcher().watch(path, RecursiveMode::Recursive)?;
    std::mem::forget(bouncer);

    thread::spawn(move || loop {
        let ev = rx.recv().expect("sender should not have deallocated");
        if let Ok(ev) = ev {
            if !ev.is_empty() {
                event_sender
                    .send(ExternalEvent::RefreshFiletree)
                    .expect("receiver should not have been deallocated");
            }
        };
    });

    Ok(())
}
