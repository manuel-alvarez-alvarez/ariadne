//! The daemon's own log, kept in memory.
//!
//! A `tracing_subscriber` [`Layer`] turns every log event into a structured
//! [`LogLineDto`] and hands it to a [`LogBuffer`]: a bounded ring of recent
//! lines for the `/v1/logs` snapshot, plus a broadcast channel fanning new
//! lines out to `/v1/logs/stream` followers — the same shape as the
//! [`EventBus`](crate::bus::EventBus). stdout logging is untouched: the layer
//! runs alongside the fmt subscriber, behind the same env filter.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use chrono::SecondsFormat;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::Context;

use ariadne_api::logs::LogLineDto;

/// Lines the ring buffer holds before evicting the oldest.
const CAPACITY: usize = 2000;
/// Lines buffered per stream follower before it is considered too slow.
const CHANNEL_CAPACITY: usize = 256;

/// Recent daemon log lines, held by [`AppState`](crate::http::AppState).
#[derive(Clone)]
pub struct LogBuffer {
    lines: Arc<Mutex<VecDeque<LogLineDto>>>,
    capacity: usize,
    tx: broadcast::Sender<LogLineDto>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::with_capacity(CAPACITY)
    }

    /// A buffer holding at most `capacity` lines. Tests use a small one to
    /// exercise eviction without logging two thousand lines.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            lines: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
            tx: broadcast::Sender::new(CHANNEL_CAPACITY),
        }
    }

    /// The [`Layer`](tracing_subscriber::Layer) feeding this buffer; register
    /// it alongside the fmt layer.
    pub fn layer(&self) -> CaptureLayer {
        CaptureLayer {
            buffer: self.clone(),
        }
    }

    /// Append a line, evicting the oldest past capacity, and broadcast it.
    fn push(&self, line: LogLineDto) {
        let mut lines = self.lines.lock().unwrap();
        if lines.len() == self.capacity {
            lines.pop_front();
        }
        lines.push_back(line.clone());
        // Sent under the lock, so a follower subscribed via
        // `snapshot_and_follow` sees every line exactly once.
        let _ = self.tx.send(line);
    }

    /// The buffered lines, oldest first.
    pub fn snapshot(&self) -> Vec<LogLineDto> {
        self.lines.lock().unwrap().iter().cloned().collect()
    }

    /// The buffered lines plus a subscription to everything after them —
    /// taken atomically against [`push`](Self::push), so a stream opened here
    /// neither misses a line nor sees one twice across the boundary.
    pub fn snapshot_and_follow(&self) -> (Vec<LogLineDto>, broadcast::Receiver<LogLineDto>) {
        let lines = self.lines.lock().unwrap();
        (lines.iter().cloned().collect(), self.tx.subscribe())
    }
}

/// Feeds every `tracing` event it sees into a [`LogBuffer`].
pub struct CaptureLayer {
    buffer: LogBuffer,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let metadata = event.metadata();
        self.buffer.push(LogLineDto {
            ts: chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message: fields.into_message(),
        });
    }
}

/// Renders an event's fields the way the fmt layer does: the `message` field
/// first, every other field appended as ` key=value`.
#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Fields {
    fn into_message(self) -> String {
        self.message + &self.rest
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            let _ = write!(self.rest, " {}={}", field.name(), value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            let _ = write!(self.rest, " {}={:?}", field.name(), value);
        }
    }
}
