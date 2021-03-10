use std::sync::RwLock;
use std::sync::RwLockReadGuard;

use chrono::Utc;

#[derive(Debug, Serialize)]
pub enum EventVariant {
    Ping,
    Pull,
    Build,
    Restart,
}

#[derive(Debug, Serialize)]
pub struct Event {
    variant: EventVariant,
    message: Option<String>,
}

impl From<EventVariant> for Event {
    fn from(variant: EventVariant) -> Self {
        Self {
            variant,
            message: None,
        }
    }
}

impl Event {
    pub fn with_message(variant: EventVariant, message: String) -> Self {
        Self {
            variant,
            message: Some(message),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TimestampedEvent {
    timestamp: i64,
    event: Event,
}

impl TimestampedEvent {
    pub fn new(event: Event) -> Self {
        // Get the current timestamp
        let timestamp = Utc::now().timestamp();

        Self { timestamp, event }
    }
}

type Queue = Vec<TimestampedEvent>;

#[derive(Debug, Default, Serialize)]
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
