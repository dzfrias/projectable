use log::{Level, Log, Metadata, Record};
use std::sync::{Arc, Mutex};

pub static EVENT_LOGGER: EventLogger = EventLogger::new();

/// Simple logger that pushes logs to a set of subscribers
#[derive(Default)]
pub struct EventLogger {
    subscribers: Mutex<Vec<Arc<dyn EventLoggerSubscriber>>>,
}

impl EventLogger {
    pub const fn new() -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe(&self, subscriber: Arc<dyn EventLoggerSubscriber>) {
        self.subscribers.lock().unwrap().push(subscriber);
    }
}

impl Log for EventLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let s = record.args().to_string();
        for subscriber in self.subscribers.lock().unwrap().iter() {
            subscriber.receive(&s, record.level());
        }
    }

    fn flush(&self) {}
}

pub trait EventLoggerSubscriber: Send + Sync {
    fn receive(&self, message: &str, level: Level);
}
