use super::ExternalEvent;
use crossbeam_channel::Sender;
use crossterm::event;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum PollState {
    #[default]
    Polling,
    Paused,
}

/// Watch for crossterm events in a separate thread, sending results to `event_sender`
pub fn crossterm_watch(event_sender: Sender<ExternalEvent>) -> Arc<Mutex<PollState>> {
    // Created to stop polling when input should be replaced: i.e. EDITOR is opened
    let poll_state = Arc::new(Mutex::new(PollState::Polling));
    const POLL_TIME: u64 = 100;

    {
        let poll_state = Arc::clone(&poll_state);
        thread::spawn(move || loop {
            // Non-blocking event read
            match event::poll(Duration::from_millis(POLL_TIME)) {
                Ok(can_poll) => {
                    if !can_poll
                        || *poll_state.lock().expect("error locking mutex") == PollState::Paused
                    {
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

    poll_state
}
