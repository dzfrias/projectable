use std::{
    path::Path,
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};

use super::EventType;
use anyhow::Result;
use notify::RecursiveMode;

pub fn fs_watch(path: &Path, event_sender: Sender<EventType>) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut bouncer = notify_debouncer_mini::new_debouncer(Duration::from_secs(1), None, tx)?;
    bouncer.watcher().watch(path, RecursiveMode::Recursive)?;
    std::mem::forget(bouncer);

    thread::spawn(move || loop {
        let ev = rx.recv().expect("sender should not have deallocated");
        if let Ok(ev) = ev {
            if !ev.is_empty() {
                event_sender
                    .send(EventType::RefreshFiletree)
                    .expect("receiver should not have been deallocated");
            }
        };
    });

    Ok(())
}
