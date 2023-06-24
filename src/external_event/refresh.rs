use super::{ExternalEvent, RefreshData};
use anyhow::Result;
use crossbeam_channel::{unbounded, Sender};
use notify::{recommended_watcher, Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct ChangeBuffer {
    create_buf: Arc<Mutex<Vec<PathBuf>>>,
    remove_buf: Arc<Mutex<Vec<PathBuf>>>,
}

impl ChangeBuffer {
    pub fn new() -> Self {
        Self {
            create_buf: Arc::new(Mutex::new(Vec::new())),
            remove_buf: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add_created(&mut self, path: Vec<PathBuf>) {
        self.create_buf
            .lock()
            .expect("failed to lock create buffer")
            .extend(path);
    }

    pub fn add_removed(&mut self, path: Vec<PathBuf>) {
        self.remove_buf
            .lock()
            .expect("failed to lock remove buffer")
            .extend(path);
    }

    pub fn flush(&mut self, sender: &Sender<ExternalEvent>) {
        macro_rules! send_buffer {
            ($buffer:expr, $refresh_type:ident) => {
                if !$buffer.is_empty() {
                    let res = sender.send(ExternalEvent::PartialRefresh(
                        $buffer.drain(..).map(RefreshData::$refresh_type).collect(),
                    ));
                    if let Err(err) = res {
                        sender
                            .send(ExternalEvent::Error(err.into()))
                            .expect("sending error failed");
                    }
                }
            };
        }

        send_buffer!(self.create_buf.lock().unwrap(), Add);
        send_buffer!(self.remove_buf.lock().unwrap(), Delete);
    }
}

/// Watch for changes to the filesystem at `path`, sending results to `event_sender`
pub fn fs_watch(
    path: &Path,
    event_sender: Sender<ExternalEvent>,
    refresh_time: u64,
    is_suspended: Arc<AtomicBool>,
) -> Result<(RecommendedWatcher, ChangeBuffer)> {
    let (tx, rx) = unbounded();
    let mut watcher = recommended_watcher(tx)?;
    watcher.configure(Config::default().with_poll_interval(Duration::from_millis(refresh_time)))?;
    watcher.watch(path, RecursiveMode::Recursive)?;
    let buffer = ChangeBuffer::new();
    let mut thread_buffer = buffer.clone();
    thread::spawn(move || {
        for res in rx {
            let send_result: Result<()> = match res {
                Ok(event) => match event.kind {
                    EventKind::Create(_) => {
                        if is_suspended.load(Ordering::Acquire) {
                            thread_buffer.add_created(event.paths);
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
                            thread_buffer.add_removed(event.paths);
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

    Ok((watcher, buffer))
}
