//! Folding the write-ahead log back into the database, off the commit path.
//!
//! Both stores turn SQLite's automatic checkpoint off, because it runs on the
//! connection that committed and cannot finish while any reader is on an
//! older snapshot. Left on, a WAL stays above the threshold and every commit
//! pays for a checkpoint that cannot finish through its single writer.
//!
//! So it happens here instead: on a clock, on a task of its own, where taking
//! a moment costs nobody their write.

use std::time::Duration;

use tracing::{debug, warn};

use ariadne_knowledge::KnowledgeStore;
use ariadne_store::Store;

/// Start folding both logs back in, every `every`, until the daemon ends.
pub fn start(store: Store, knowledge: Option<KnowledgeStore>, every: Duration) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick is immediate and there is nothing to fold in yet.
        tick.tick().await;
        loop {
            tick.tick().await;
            checkpoint_tick(&store, knowledge.as_ref()).await;
        }
    });
}

async fn checkpoint_tick(store: &Store, knowledge: Option<&KnowledgeStore>) {
    match store.checkpoint().await {
        Ok(()) => debug!(store = "daemon", "folded the write-ahead log back in"),
        // Readers held it, or the store is closing. Either way the next one
        // folds in what this one could not, so it is said once and not raised.
        Err(error) => warn!(store = "daemon", %error, "could not fold the write-ahead log back in"),
    }
    if let Some(knowledge) = knowledge {
        match knowledge.checkpoint().await {
            Ok(()) => debug!(store = "knowledge", "folded the write-ahead log back in"),
            Err(error) => {
                warn!(store = "knowledge", %error, "could not fold the write-ahead log back in")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_knowledge::State;

    #[tokio::test]
    async fn a_checkpoint_tick_bounds_the_knowledge_write_ahead_log() {
        const WAL_BOUND: u64 = 32 * 1024;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("ariadne.db")).await.unwrap();
        let path = dir.path().join("knowledge.db");
        let knowledge = KnowledgeStore::open(&path).await.unwrap();
        let wal = path.with_extension("db-wal");

        for n in 0..200 {
            knowledge
                .set_ref_state(&format!("repo-{n}"), "main", State::Idle, None)
                .await
                .unwrap();
        }
        let before = std::fs::metadata(&wal).map(|file| file.len()).unwrap_or(0);
        assert!(before > WAL_BOUND, "the writes made a {before}-byte WAL");

        checkpoint_tick(&store, Some(&knowledge)).await;

        let after = std::fs::metadata(&wal).map(|file| file.len()).unwrap_or(0);
        assert!(
            after < WAL_BOUND,
            "the checkpoint left a {after}-byte knowledge WAL"
        );
    }
}
