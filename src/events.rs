use std::sync::RwLock;
use std::{fmt, sync::RwLockReadGuard};

use chrono::Utc;

#[derive(Debug)]
pub enum Event {
    Ping,
    Pull,
    Build(String),
    Restart(String),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Ping => write!(f, "ping"),
            Event::Pull => write!(f, "pull"),
            Event::Build(s) => write!(f, "build: {}", s),
            Event::Restart(s) => write!(f, "restart: {}", s),
        }
    }
}

#[derive(Debug)]
pub struct TimestampedEvent {
    timestamp: i64,
    event: Event,
}

impl fmt::Display for TimestampedEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.timestamp, self.event)
    }
}

impl TimestampedEvent {
    pub fn new(event: Event) -> Self {
        // Get the current timestamp
        let timestamp = Utc::now().timestamp();

        Self { timestamp, event }
    }
}

type Queue = Vec<TimestampedEvent>;

#[derive(Debug, Default)]
pub struct TimeseriesQueue {
    queue: RwLock<Queue>,
}

impl TimeseriesQueue {
    pub fn push(&self, event: Event) {
        let timestamped = TimestampedEvent::new(event);
        let mut writer = self.queue.write().unwrap();

        writer.push(timestamped);
    }

    pub fn read(&self) -> RwLockReadGuard<'_, Queue> {
        self.queue.read().unwrap()
    }
}
