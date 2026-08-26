//! One session's terminal: what to read, and when.
//!
//! tmux `pipe-pane` appends every byte a session's pane produces to
//! `<run_dir>/<session>/console.log` (see
//! [`crate::tmux::TmuxManager::new_session`]), so following a session's
//! terminal is reading whatever was appended since the last read. That beats
//! re-capturing and diffing the pane: it costs no process spawn, and it keeps
//! the raw byte stream — escape sequences included — which `capture-pane`
//! throws away.
//!
//! [`LogTail`] is the cursor into that file; [`LogWatch`] is what says there
//! is something to read. Asking on a timer instead would put the whole
//! interval between a keystroke's echo landing in the file and the frame that
//! carries it, which is felt directly as sluggish typing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::mpsc;
use tracing::warn;

/// The most one [`LogTail::read_new`] hands back.
///
/// An agent that dumps a large file, or a pane redrawing itself over and over
/// while nobody reads it, can leave megabytes appended between two reads —
/// and an SSE frame is written, buffered and parsed whole. Capping what a
/// read yields bounds what one stream holds at a time and lets a client
/// render a burst as it arrives instead of at the end of it. Nothing is
/// dropped: the remainder is what the next read starts with, which
/// [`LogTail::has_backlog`] reports so a caller can come straight back for
/// it.
///
/// The cap is on the *decoded* output, not on the bytes read to produce it:
/// lossy decoding turns every byte that is not valid UTF-8 into a three-byte
/// replacement character, so a capped read of a binary file would otherwise
/// yield three times the cap.
pub const MAX_CHUNK: usize = 256 * 1024;

/// A cursor into one console log.
pub struct LogTail {
    path: PathBuf,
    offset: u64,
    /// Trailing bytes of an incomplete UTF-8 character: a read can land in the
    /// middle of one, and half a character is worth carrying rather than
    /// decoding into a replacement glyph.
    pending: Vec<u8>,
    /// Decoded output the frame cap held back, to be handed out by the reads
    /// that follow before anything new is read from the file.
    carry: String,
    /// Whether the file already holds bytes past where the last read stopped.
    more: bool,
}

impl LogTail {
    /// A tail positioned at the start of `path`. The file need not exist yet.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            pending: Vec::new(),
            carry: String::new(),
            more: false,
        }
    }

    /// Where the file ends right now.
    ///
    /// For a caller that is about to capture the screen: the tail has to be
    /// marked *before* the capture, so that whatever is written in between is
    /// sent twice rather than not at all — but a capture can fail, and then
    /// the tail must not have moved at all. Reading the mark and acting on it
    /// are separate for that reason (see [`Self::skip_to`]).
    pub async fn end_offset(&self) -> u64 {
        tokio::fs::metadata(&self.path)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Drop everything before `offset`, half-read character and output the
    /// frame cap held back included.
    pub fn skip_to(&mut self, offset: u64) {
        self.offset = offset;
        self.pending.clear();
        self.carry.clear();
        self.more = false;
    }

    /// Position back at the start of the file.
    pub fn rewind(&mut self) {
        self.skip_to(0);
    }

    /// Whether there is output to be had without waiting: held back by the
    /// frame cap, or already in the file past where the last read stopped.
    ///
    /// Only what has been written already is counted, so a `false` says
    /// nothing about what is appended next — it means "no reason to read
    /// again this instant", not "the pane is quiet".
    pub fn has_backlog(&self) -> bool {
        self.more || !self.carry.is_empty()
    }

    /// What was appended since the last read, decoded lossily, up to
    /// [`MAX_CHUNK`] of it. Empty when the file is absent, unchanged, or
    /// unreadable; [`Self::has_backlog`] says whether the cap is what ended
    /// it.
    pub async fn read_new(&mut self) -> String {
        if self.carry.is_empty() {
            self.fill().await;
        }
        self.take_frame()
    }

    /// Everything that is left, for when nothing more will ever be appended:
    /// what the cap is holding back, the last unread bytes, and any
    /// half-written character carried from an earlier read — that last one
    /// decoded lossily this time. Nothing is held back: this is the one read
    /// whose caller has no next one to come back on.
    pub async fn drain(&mut self) -> String {
        let mut out = self.read_new().await;
        while self.has_backlog() {
            out.push_str(&self.read_new().await);
        }
        if !self.pending.is_empty() {
            let leftover = std::mem::take(&mut self.pending);
            out.push_str(&String::from_utf8_lossy(&leftover));
        }
        out
    }

    /// Read the next stretch of the file into [`Self::carry`], decoded.
    ///
    /// Bounded twice over: at most a cap's worth of *bytes* is read, so that
    /// a quiet stream never pays for a burst it did not ask for, and what
    /// that decodes to is then handed out a frame at a time by
    /// [`Self::take_frame`].
    async fn fill(&mut self) {
        self.more = false;
        let Ok(mut file) = tokio::fs::File::open(&self.path).await else {
            // Not there (yet): pipe-pane creates it as the session starts.
            return;
        };
        let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Truncated or replaced under us: re-read rather than sit past EOF.
            self.rewind();
        }
        if len == self.offset {
            return;
        }
        if let Err(e) = file.seek(SeekFrom::Start(self.offset)).await {
            warn!(path = %self.path.display(), error = %e, "seeking console log failed");
            return;
        }
        // A carried half-character is decoded with what follows it, so it
        // counts against the same bound.
        let want = (len - self.offset).min((MAX_CHUNK - self.pending.len()) as u64);
        let mut buf = Vec::with_capacity(want as usize);
        let read = match file.take(want).read_to_end(&mut buf).await {
            Ok(n) => {
                self.offset += n as u64;
                n
            }
            Err(e) => {
                warn!(path = %self.path.display(), error = %e, "reading console log failed");
                return;
            }
        };
        // A read that came back with nothing — the file shrank under it, say
        // — reports no backlog whatever the length said, so that a caller
        // reading until there is none cannot be made to spin.
        self.more = read > 0 && self.offset < len;
        self.carry = self.decode(buf);
    }

    /// A frame's worth of what has been decoded, cut on a character boundary.
    fn take_frame(&mut self) -> String {
        if self.carry.len() <= MAX_CHUNK {
            return std::mem::take(&mut self.carry);
        }
        let mut end = MAX_CHUNK;
        while !self.carry.is_char_boundary(end) {
            end -= 1;
        }
        let rest = self.carry.split_off(end);
        std::mem::replace(&mut self.carry, rest)
    }

    fn decode(&mut self, bytes: Vec<u8>) -> String {
        let buf = if self.pending.is_empty() {
            bytes
        } else {
            let mut carried = std::mem::take(&mut self.pending);
            carried.extend_from_slice(&bytes);
            carried
        };
        match std::str::from_utf8(&buf) {
            Ok(text) => text.to_owned(),
            // `error_len() == None` means the input simply ends mid-character,
            // so the remainder is the start of a character still being written.
            Err(e) if e.error_len().is_none() => {
                let valid = e.valid_up_to();
                self.pending = buf[valid..].to_vec();
                String::from_utf8_lossy(&buf[..valid]).into_owned()
            }
            // Genuinely not UTF-8: replace, and keep the stream going.
            Err(_) => String::from_utf8_lossy(&buf).into_owned(),
        }
    }
}

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
///
/// The watch is on the log file itself, which rules out FSEvents, macOS's
/// default backend and `notify`'s: it reports a file as modified when the
/// writer closes it, and `pipe-pane` holds the log open for the whole of a
/// session — so an FSEvents watch on a live console log is silent for hours.
/// kqueue's `NOTE_WRITE` and inotify's `IN_MODIFY` both fire on the write
/// itself, which is why the daemon asks for the kqueue backend on macOS (see
/// the `notify` dependency in the workspace manifest).
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
        // file, so "something changed" does not accumulate.
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
    /// `pipe-pane` creates it with the session's first output, and a
    /// directory event says "something happened here", which is all that is
    /// needed to go and look. Cheap enough to attempt on every wait: it is
    /// one `stat` short of a watch already on the log.
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

    async fn append(path: &Path, bytes: &[u8]) {
        write(&mut appender(path).await, bytes).await;
    }

    async fn write(file: &mut tokio::fs::File, bytes: &[u8]) {
        file.write_all(bytes).await.unwrap();
        file.flush().await.unwrap();
    }

    fn log_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("console.log")
    }

    /// Read frame by frame to the end of the backlog: what a stream does with
    /// a burst bigger than one frame. Every frame is checked against the cap
    /// on the way past.
    async fn read_frames(tail: &mut LogTail) -> (String, usize) {
        let (mut read, mut frames) = (String::new(), 0);
        loop {
            let frame = tail.read_new().await;
            if frame.is_empty() {
                break;
            }
            assert!(frame.len() <= MAX_CHUNK, "over the cap: {}", frame.len());
            frames += 1;
            read.push_str(&frame);
            if !tail.has_backlog() {
                break;
            }
        }
        assert!(!tail.has_backlog(), "nothing left behind");
        (read, frames)
    }

    #[tokio::test]
    async fn reads_only_what_was_appended() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        append(&log, b"hello\n").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "hello\n");
        assert_eq!(tail.read_new().await, "", "nothing new yet");

        append(&log, b"\x1b[2Jworld\r\n").await;
        assert_eq!(tail.read_new().await, "\x1b[2Jworld\r\n");
    }

    /// The cursor goes where it is put: past a snapshot the client already
    /// has, and back to the start when the whole file is to be replayed.
    #[tokio::test]
    async fn the_cursor_can_be_moved_to_the_end_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        append(&log, b"already rendered\n").await;

        let mut tail = LogTail::new(&log);
        let end = tail.end_offset().await;
        tail.skip_to(end);
        assert_eq!(tail.read_new().await, "");

        append(&log, b"new\n").await;
        assert_eq!(tail.read_new().await, "new\n");

        tail.rewind();
        assert_eq!(tail.read_new().await, "already rendered\nnew\n");
    }

    /// pipe-pane only creates the file once the session produces output.
    #[tokio::test]
    async fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);

        let mut tail = LogTail::new(&log);
        let end = tail.end_offset().await;
        tail.skip_to(end);
        assert_eq!(tail.read_new().await, "");

        append(&log, b"late\n").await;
        assert_eq!(tail.read_new().await, "late\n");
    }

    #[tokio::test]
    async fn truncation_restarts_from_the_beginning() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        append(&log, b"first run\n").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "first run\n");

        tokio::fs::write(&log, b"fresh\n").await.unwrap();
        assert_eq!(tail.read_new().await, "fresh\n");
    }

    /// A read can land in the middle of a multi-byte character; the halves
    /// must join up instead of turning into replacement glyphs.
    #[tokio::test]
    async fn a_character_split_across_reads_survives() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        // "│" is three bytes; box drawing is all over agent TUIs.
        let bar = "│".as_bytes();
        append(&log, b"a").await;
        append(&log, &bar[..2]).await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "a");

        append(&log, &bar[2..]).await;
        append(&log, b"b").await;
        assert_eq!(tail.read_new().await, "│b");
    }

    /// Nothing more is coming, so the half-written character has to go out
    /// lossily rather than be swallowed.
    #[tokio::test]
    async fn drain_flushes_a_half_written_character() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        let bar = "│".as_bytes();
        append(&log, b"cut off: ").await;
        append(&log, &bar[..2]).await;

        let mut tail = LogTail::new(&log);
        assert_eq!(
            tail.read_new().await,
            "cut off: ",
            "the partial character is held back while more may still arrive"
        );
        assert_eq!(tail.drain().await, "\u{fffd}");
        assert_eq!(tail.drain().await, "", "nothing is left to flush twice");
    }

    /// A burst bigger than a frame comes back in frames, in order, whole —
    /// and the cap gives way for a drain, whose caller has no next read.
    #[tokio::test]
    async fn a_large_append_is_read_a_frame_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        let burst: Vec<u8> = (0..MAX_CHUNK * 2 + 1_234)
            .map(|i| b'a' + (i % 26) as u8)
            .collect();
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        let (read, frames) = read_frames(&mut tail).await;
        assert_eq!(frames, 3);
        assert_eq!(read.as_bytes(), burst.as_slice(), "in order and whole");

        tail.rewind();
        assert_eq!(
            tail.drain().await.len(),
            burst.len(),
            "a drain caps nothing"
        );
        assert_eq!(tail.drain().await, "");
    }

    /// The cap must not fall inside a character: the frame stops short and
    /// the remainder joins the next one, exactly as a torn read does.
    #[tokio::test]
    async fn a_character_straddling_the_cap_survives() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        // The frame's last byte falls inside a three-byte character.
        let mut burst = vec![b'x'; MAX_CHUNK - 1];
        burst.extend_from_slice("\u{2502}tail".as_bytes());
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        let first = tail.read_new().await;
        assert_eq!(
            first.len(),
            MAX_CHUNK - 1,
            "the frame stops before the half-read character"
        );
        assert!(tail.has_backlog());
        assert_eq!(tail.read_new().await, "\u{2502}tail");
    }

    /// Lossy decoding is not length-preserving: every byte that is not valid
    /// UTF-8 becomes a three-byte replacement character, so a cap on the
    /// bytes *read* would let a frame out at three times the size. The cap is
    /// on what comes back — and nothing is dropped in the splitting.
    #[tokio::test]
    async fn invalid_bytes_cannot_inflate_a_frame_past_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        // A cap's worth of bytes that are all invalid on their own: three
        // caps' worth of replacement characters.
        let burst = vec![0xffu8; MAX_CHUNK];
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        let (read, frames) = read_frames(&mut tail).await;
        assert!(
            frames >= 3,
            "one byte read decodes to three, so a capped read is three \
             frames and a remainder: {frames}"
        );
        assert_eq!(read, String::from_utf8_lossy(&burst));
    }

    #[tokio::test]
    async fn an_append_wakes_the_waiter_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = log_in(&dir);
        let mut writer = appender(&log).await;
        write(&mut writer, b"already there\n").await;

        let mut watch = LogWatch::new(&log);
        // Arms the watch, and returns on the budget: nothing has changed.
        watch.changed(Duration::from_millis(50)).await;

        // Three in a row, all through the one open handle: the wake-up must
        // not depend on the writer letting go of the file.
        for i in 0..3 {
            write(&mut writer, format!("line {i}\n").as_bytes()).await;
            let started = Instant::now();
            watch.changed(Duration::from_secs(5)).await;
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_millis(100),
                "the write, not the budget, is what ended the wait: {elapsed:?}"
            );
        }
    }

    /// The budget is the caller's fallback, and it has to hold whenever
    /// nothing is written — a quiet pane must not spin, and neither must a
    /// viewer that connected before the session's run dir existed at all.
    #[tokio::test]
    async fn a_quiet_log_waits_out_its_budget() {
        let dir = tempfile::tempdir().unwrap();
        let quiet = log_in(&dir);
        append(&quiet, b"quiet\n").await;
        let nowhere = dir.path().join("not-yet").join("console.log");

        for path in [quiet, nowhere] {
            let mut watch = LogWatch::new(&path);
            let started = Instant::now();
            watch.changed(Duration::from_millis(200)).await;
            assert!(
                started.elapsed() >= Duration::from_millis(200),
                "{}",
                path.display()
            );
        }
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
                append(&log, b"first output\n").await;
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
        write(&mut writer, b"more output\n").await;
        let started = Instant::now();
        watch.changed(Duration::from_secs(5)).await;
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "the watch moved onto the log: {elapsed:?}"
        );
    }
}
