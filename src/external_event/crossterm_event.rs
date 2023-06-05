use super::ExternalEvent;
use crossbeam_channel::Sender;
use crossterm::event;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Watch for crossterm events in a separate thread, sending results to `event_sender`
pub fn crossterm_watch(
    event_sender: Sender<ExternalEvent>,
    stop_flag: Arc<AtomicBool>,
) -> JoinHandle<()> {
    // Created to stop polling when input should be replaced: i.e. EDITOR is opened
    const POLL_TIME: u64 = 100;

    thread::spawn(move || loop {
        // Allow thread to be joined if `stop_flag` is set to true from another thread
        if stop_flag.load(Ordering::Acquire) {
            break;
        }

        // Non-blocking event read
        match event::poll(Duration::from_millis(POLL_TIME)) {
            Ok(can_poll) => {
                if !can_poll {
                    continue;
                }
                match event::read() {
                    Ok(ev) => event_sender
                        .send(ExternalEvent::Crossterm(ev))
                        .expect("receiver should not have been deallocated"),
                    Err(err) => event_sender
                        .send(ExternalEvent::Error(err.into()))
                        .expect("receiver should not have been deallocated"),
                }
            }
            Err(err) => {
                event_sender
                    .send(ExternalEvent::Error(err.into()))
                    .expect("receiver should not have been deallocated");
            }
        }
    })
}
