//! Wake-ups for an append-only console log.
//!
//! Following a session's terminal is reading whatever tmux `pipe-pane`
//! appended to `<run_dir>/<session>/console.log` (see [`crate::logtail`]) —
//! the only question is *when* to read. Asking on a timer puts the whole
//! interval between a keystroke's echo landing in the file and the frame that
//! carries it, which is felt directly as sluggish typing. Asking the kernel
//! instead costs nothing while the pane is quiet and answers within a
//! millisecond or two when it is not.
//!
//! The watch is on the log file itself. That rules out FSEvents, macOS's
//! default backend and `notify`'s: it reports a file as modified when the
//! writer closes it, and `pipe-pane` holds the log open for the whole of a
//! session — so an FSEvents watch on a live console log is silent for hours.
//! kqueue's `NOTE_WRITE` and inotify's `IN_MODIFY` both fire on the write
//! itself, which is why the daemon asks for the kqueue backend on macOS (see
//! the `notify` dependency in the workspace manifest).
//!
//! Until the log exists — `pipe-pane` creates it with the session's first
//! output — there is nothing to open, so the directory it will appear in is
//! watched instead. A directory event says only "something happened here",
//! which is all that is needed to go and look.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::warn;

/// How long a wake-up is held before the log is read.
///
/// A pane redrawing itself writes many times in quick succession, and one
/// frame per `write(2)` would be both wasteful and, past a few hundred a
/// second, worse for the client than the batch it could have had. Waiting
/// this long lets a burst coalesce into a single `delta` while staying far
/// inside what a typist can feel.
const DEBOUNCE: Duration = Duration::from_millis(10);

/// What a watch is actually on.
#[derive(Clone, Copy, PartialEq)]
enum Watched {
    /// The log itself: every event is about the output being followed.
    Log,
    /// The directory the log has yet to appear in — a stand-in until it does.
    Dir,
}

/// A subscription to one console log's writes.
pub struct LogWatch {
    file: PathBuf,
    dir: PathBuf,
    /// Handed to the watcher's callback, which runs on the watcher's own
    /// thread; kept here to establish the watch later, or to move it onto the
    /// log once there is one.
    tx: mpsc::Sender<()>,
    rx: mpsc::Receiver<()>,
    /// The live watch, if one could be established, and what it is on.
    /// Dropping it ends it, so it is held for as long as the follower it
    /// wakes.
    watcher: Option<(RecommendedWatcher, Watched)>,
}

impl LogWatch {
    /// A watch on `file`. Neither it nor its directory need exist yet: what
    /// can be watched is established here if it is there, on the first wait
    /// that finds it otherwise, and until then callers simply fall back on
    /// their own timeout.
    ///
    /// Watching starts now rather than at the first wait so that there is no
    /// window in which the log is written to with nobody listening: a write
    /// that lands between a caller's last read and its next wait has to leave
    /// a wake-up behind, or it waits out a whole budget for output that was
    /// already there.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        let file = file.into();
        let dir = file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        // One slot is all a wake-up needs: the reader reads to the end of the
        // file, so "something changed" does not accumulate. A send that finds
        // the slot taken has nothing to add to the signal already waiting.
        let (tx, rx) = mpsc::channel(1);
        let mut watch = Self {
            file,
            dir,
            tx,
            rx,
            watcher: None,
        };
        watch.arm();
        watch
    }

    /// Wait until the log is written to, or until `budget` runs out.
    ///
    /// Returning is a hint and never a promise: a spurious wake-up costs the
    /// caller one read that finds nothing, and a missed one costs it the rest
    /// of its budget. Callers therefore pass a budget they are happy to be
    /// woken by anyway — the follower's is its next liveness check — so that
    /// output is never *lost* to a watch that could not be established or did
    /// not fire, only delayed to what the old fixed poll cost it every time.
    pub async fn changed(&mut self, budget: Duration) {
        if budget.is_zero() {
            return;
        }
        self.arm();
        // Something landed while the last chunk was being sent: no reason to
        // wait for the next write to say so again.
        if self.rx.try_recv().is_err()
            && tokio::time::timeout(budget, self.rx.recv()).await.is_err()
        {
            return;
        }
        tokio::time::sleep(DEBOUNCE.min(budget)).await;
        // Whatever else arrived while it settled is covered by the read this
        // returns into, so it must not wake the next wait as well.
        while self.rx.try_recv().is_ok() {}
    }

    /// Put the watch on the log, or on its directory while there is no log —
    /// and move it across once there is. Cheap enough to attempt on every
    /// wait: it is one `stat` short of a watch already on the log.
    fn arm(&mut self) {
        let target = if self.file.is_file() {
            Watched::Log
        } else if self.dir.is_dir() {
            Watched::Dir
        } else {
            return;
        };
        if self.watcher.as_ref().is_some_and(|(_, on)| *on == target) {
            return;
        }
        let path = match target {
            Watched::Log => &self.file,
            Watched::Dir => &self.dir,
        };
        let tx = self.tx.clone();
        let handler = move |event: notify::Result<notify::Event>| match event {
            // Unfiltered: a watch on the log has nothing else to report, and
            // a watch on the run dir reports the handful of small files the
            // session keeps there — one wasted read apiece, against the cost
            // of matching paths a backend may have resolved through a symlink
            // on the way back.
            Ok(_) => {
                let _ = tx.try_send(());
            }
            // A watch that breaks leaves the follower on its own budget,
            // which is the behaviour it had before there was one at all.
            Err(e) => warn!(error = %e, "watching the console log failed"),
        };
        match notify::recommended_watcher(handler)
            .and_then(|mut w| w.watch(path, RecursiveMode::NonRecursive).map(|()| w))
        {
            // Assigning drops the watch this replaces, if any.
            Ok(watcher) => self.watcher = Some((watcher, target)),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "cannot watch for console log output")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Instant;

    use tokio::io::AsyncWriteExt;

    /// Appends through a handle that stays open, which is how `pipe-pane`
    /// writes a console log — and the case FSEvents does not report.
    async fn appender(path: &Path) -> tokio::fs::File {
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .unwrap()
    }

    async fn append(file: &mut tokio::fs::File, bytes: &[u8]) {
        file.write_all(bytes).await.unwrap();
        file.flush().await.unwrap();
    }

    #[tokio::test]
    async fn an_append_wakes_the_waiter_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        let mut writer = appender(&log).await;
        append(&mut writer, b"already there\n").await;

        let mut watch = LogWatch::new(&log);
        // Arms the watch, and returns on the budget: nothing has changed.
        watch.changed(Duration::from_millis(50)).await;

        // Three in a row, all through the one open handle: the wake-up must
        // not depend on the writer letting go of the file.
        for i in 0..3 {
            append(&mut writer, format!("line {i}\n").as_bytes()).await;
            let started = Instant::now();
            watch.changed(Duration::from_secs(5)).await;
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_millis(100),
                "the write, not the budget, is what ended the wait: {elapsed:?}"
            );
        }
    }

    /// The budget is the caller's fallback, and it has to hold when nothing
    /// is written — a quiet pane must not spin.
    #[tokio::test]
    async fn a_quiet_log_waits_out_its_budget() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&mut appender(&log).await, b"quiet\n").await;

        let mut watch = LogWatch::new(&log);
        let started = Instant::now();
        watch.changed(Duration::from_millis(200)).await;
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    /// pipe-pane creates the log only once the session writes. Until then the
    /// directory stands in for it, and the watch moves onto the log itself as
    /// soon as there is one.
    #[tokio::test]
    async fn a_log_that_does_not_exist_yet_is_watched_once_it_appears() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path().join("session");
        std::fs::create_dir_all(&run_dir).unwrap();
        let log = run_dir.join("console.log");

        let mut watch = LogWatch::new(&log);
        let waiting = {
            let log = log.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                append(&mut appender(&log).await, b"first output\n").await;
            })
        };
        let started = Instant::now();
        watch.changed(Duration::from_secs(5)).await;
        let elapsed = started.elapsed();
        waiting.await.unwrap();
        assert!(
            elapsed < Duration::from_millis(500),
            "the log appearing in the watched directory is a wake-up: {elapsed:?}"
        );

        // And the watch has followed the log, so what is written to it now
        // wakes the waiter as directly as ever.
        let mut writer = appender(&log).await;
        watch.changed(Duration::from_millis(50)).await;
        append(&mut writer, b"more output\n").await;
        let started = Instant::now();
        watch.changed(Duration::from_secs(5)).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "the watch moved onto the log: {elapsed:?}"
        );
    }

    /// A viewer can connect before the session's run dir exists at all;
    /// there is nothing to watch and nothing to break.
    #[tokio::test]
    async fn a_directory_that_does_not_exist_yet_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("not-yet").join("console.log");

        let mut watch = LogWatch::new(&log);
        let started = Instant::now();
        watch.changed(Duration::from_millis(100)).await;
        assert!(started.elapsed() >= Duration::from_millis(100));
    }
}
