use super::EventType;
use crossterm::event;
use std::{sync::mpsc::Sender, thread, time::Duration};

pub fn crossterm_watch(event_sender: Sender<EventType>) {
    thread::spawn(move || loop {
        match event::poll(Duration::from_millis(300)) {
            Ok(can_poll) => {
                if !can_poll {
                    continue;
                }
                match event::read() {
                    Ok(ev) => event_sender
                        .send(EventType::Crossterm(ev))
                        .expect("receiver should not have been deallocated"),
                    Err(err) => event_sender
                        .send(EventType::Error(err.into()))
                        .expect("receiver should not have been deallocated"),
                }
            }
            Err(err) => {
                event_sender
                    .send(EventType::Error(err.into()))
                    .expect("receiver should not have been deallocated");
            }
        }
    });
}
