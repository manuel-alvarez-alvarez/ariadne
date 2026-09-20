//! Folding the write-ahead log back into the database, off the commit path.
//!
//! `Store::open` turns SQLite's automatic checkpoint off, because it runs on
//! the connection that committed and cannot finish while any reader is on an
//! older snapshot — and in this daemon one always is. Left on, the WAL stays
//! above the threshold and every commit pays for a checkpoint that never
//! completes, through the single write connection the whole daemon shares.
//!
//! So it happens here instead: on a clock, on a task of its own, where taking
//! a moment costs nobody their write.

use std::time::Duration;

use tracing::{debug, warn};

use ariadne_store::Store;

/// Start folding the log back in, every `every`, until the daemon ends.
pub fn start(store: Store, every: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is immediate and there is nothing to fold in yet.
        tick.tick().await;
        loop {
            tick.tick().await;
            match store.checkpoint().await {
                Ok(()) => debug!("folded the write-ahead log back in"),
                // Readers held it, or the store is closing. Either way the
                // next one folds in what this one could not, so it is said
                // once at debug and not raised.
                Err(error) => warn!(%error, "could not fold the write-ahead log back in"),
            }
        }
    });
}
