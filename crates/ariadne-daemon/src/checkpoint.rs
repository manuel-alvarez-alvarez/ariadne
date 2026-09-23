//! Folding the write-ahead log back into the database, off the commit path.
//!
//! The store turns SQLite's automatic checkpoint off, because it runs on the
//! connection that committed and cannot finish while any reader is on an
//! older snapshot. Left on, a WAL stays above the threshold and every commit
//! pays for a checkpoint that cannot finish through its single writer.
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
            checkpoint_tick(&store).await;
        }
    });
}

async fn checkpoint_tick(store: &Store) {
    match store.checkpoint().await {
        Ok(()) => debug!(store = "daemon", "folded the write-ahead log back in"),
        // Readers held it, or the store is closing. Either way the next one
        // folds in what this one could not, so it is said once and not raised.
        Err(error) => warn!(store = "daemon", %error, "could not fold the write-ahead log back in"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_store::NewRepository;

    #[tokio::test]
    async fn a_checkpoint_tick_bounds_the_write_ahead_log() {
        const WAL_BOUND: u64 = 32 * 1024;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ariadne.db");
        let store = Store::open(&path).await.unwrap();
        let wal = path.with_extension("db-wal");

        for n in 0..200 {
            store
                .create_repository(NewRepository {
                    path: format!("/tmp/repo-{n}"),
                    base_branch: "main".to_string(),
                    description: None,
                })
                .await
                .unwrap();
        }
        let before = std::fs::metadata(&wal).map(|file| file.len()).unwrap_or(0);
        assert!(before > WAL_BOUND, "the writes made a {before}-byte WAL");

        checkpoint_tick(&store).await;

        let after = std::fs::metadata(&wal).map(|file| file.len()).unwrap_or(0);
        assert!(after < WAL_BOUND, "the checkpoint left a {after}-byte WAL");
    }
}
