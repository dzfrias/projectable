use super::ExternalEvent;
use crossbeam_channel::Sender;
use crossterm::event;
use std::{thread, time::Duration};

/// Watch for crossterm events in a separate thread, sending results to `event_sender`
pub fn crossterm_watch(event_sender: Sender<ExternalEvent>) {
    const POLL_TIME: u64 = 300;

    thread::spawn(move || loop {
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
    });
}
