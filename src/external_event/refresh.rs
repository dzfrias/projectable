use super::{ExternalEvent, RefreshData};
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
            let send_result: Result<()> = match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) => event_sender
                        .send(ExternalEvent::PartialRefresh(
                            event
                                .paths
                                .into_iter()
                                .filter_map(|path| {
                                    path.try_exists()
                                        .ok()
                                        .and_then(|exists| exists.then(|| RefreshData::Add(path)))
                                })
                                .collect(),
                        ))
                        .map_err(Into::into),
                    EventKind::Remove(_) => event_sender
                        .send(ExternalEvent::PartialRefresh(
                            event
                                .paths
                                .into_iter()
                                .filter_map(|path| {
                                    path.try_exists().ok().and_then(|exists| {
                                        exists.then(|| RefreshData::Delete(path))
                                    })
                                })
                                .collect(),
                        ))
                        .map_err(Into::into),
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
