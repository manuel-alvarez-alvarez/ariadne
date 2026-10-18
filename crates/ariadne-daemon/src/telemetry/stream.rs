//! The live feed behind `GET /v1/logs/stream`: a broadcast the log
//! exporter sends every written record to, so a follower sees a line the
//! moment it lands in the telemetry store, with no polling.

use tokio::sync::broadcast;

use ariadne_api::logs::LogLineDto;

/// One committed store record, kept with its internal ID until the stream
/// compares it with its snapshot boundary. The wire DTO deliberately stays
/// free of store implementation details.
#[derive(Clone, Debug)]
pub(crate) struct StreamedLog {
    pub(crate) id: String,
    pub(crate) line: LogLineDto,
}

/// Lines buffered per stream follower before it is considered too slow —
/// the same capacity the daemon's old in-memory ring kept per follower.
const CHANNEL_CAPACITY: usize = 256;

#[derive(Clone, Debug)]
pub struct LogStream {
    tx: broadcast::Sender<StreamedLog>,
}

impl Default for LogStream {
    fn default() -> Self {
        Self::new()
    }
}

impl LogStream {
    pub fn new() -> Self {
        Self {
            tx: broadcast::Sender::new(CHANNEL_CAPACITY),
        }
    }

    /// Subscribe to every line sent from here on.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<StreamedLog> {
        self.tx.subscribe()
    }

    /// Send a line to every current follower. Non-blocking; a no-op
    /// without one.
    pub(crate) fn feed(&self, record: StreamedLog) {
        let _ = self.tx.send(record);
    }
}
