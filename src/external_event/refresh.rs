use super::{ExternalEvent, RefreshData};
use anyhow::Result;
use crossbeam_channel::{unbounded, Sender};
use notify::{recommended_watcher, Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

/// Watch for changes to the filesystem at `path`, sending results to `event_sender`
pub fn fs_watch(
    path: &Path,
    event_sender: Sender<ExternalEvent>,
    refresh_time: u64,
    is_suspended: Arc<AtomicBool>,
) -> Result<RecommendedWatcher> {
    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx)?;
    watcher.configure(Config::default().with_poll_interval(Duration::from_millis(refresh_time)))?;
    watcher.watch(path, RecursiveMode::Recursive)?;
    let mut create_buf = Vec::new();
    let mut remove_buf = Vec::new();
    thread::spawn(move || {
        for res in rx {
            if !is_suspended.load(Ordering::Acquire) && !create_buf.is_empty() {
                event_sender
                    .send(ExternalEvent::PartialRefresh(
                        create_buf.drain(..).map(RefreshData::Add).collect(),
                    ))
                    .unwrap();
            }
            if !is_suspended.load(Ordering::Acquire) && !remove_buf.is_empty() {
                event_sender
                    .send(ExternalEvent::PartialRefresh(
                        create_buf.drain(..).map(RefreshData::Delete).collect(),
                    ))
                    .unwrap();
            }
            let send_result: Result<()> = match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) => {
                        if is_suspended.load(Ordering::Acquire) {
                            create_buf.extend(event.paths);
                            Ok(())
                        } else {
                            let data = ExternalEvent::PartialRefresh(
                                event.paths.into_iter().map(RefreshData::Add).collect(),
                            );
                            event_sender.send(data).map_err(Into::into)
                        }
                    }
                    EventKind::Remove(_) => {
                        if is_suspended.load(Ordering::Acquire) {
                            remove_buf.extend(event.paths);
                            Ok(())
                        } else {
                            let data = ExternalEvent::PartialRefresh(
                                event.paths.into_iter().map(RefreshData::Delete).collect(),
                            );
                            event_sender.send(data).map_err(Into::into)
                        }
                    }
                    _ => Ok(()),
                },
                Err(e) => Err(e.into()),
            };
            if let Err(err) = send_result {
                event_sender
                    .send(ExternalEvent::Error(err))
                    .expect("sender should not have deallocated");
            }
        }
    });

    Ok(watcher)
}
