//! Incremental tail of an append-only console log.
//!
//! Every agent session is piped to `<run_dir>/<session>/console.log` by tmux
//! `pipe-pane` (see [`crate::tmux::TmuxManager::new_session`]), so following a
//! session's terminal is just reading whatever was appended since the last
//! read. That beats re-capturing and diffing the pane: it costs no process
//! spawn, and it keeps the raw byte stream — escape sequences included —
//! which `capture-pane` throws away.

use std::path::PathBuf;

use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
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
/// The cap is on the *decoded* output, not on the bytes read to produce it,
/// because those are not the same length: lossy decoding turns every byte
/// that is not valid UTF-8 into a three-byte replacement character, so a
/// capped read of a binary file would otherwise yield three times the cap.
/// It bounds one event's terminal output rather than the wire line carrying
/// it — JSON escapes control bytes, which the terminal stream is full of, so
/// the encoded `data:` line is a bounded multiple of this and not this.
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

    /// Skip whatever the file already holds — for when the caller has just
    /// sent a snapshot covering it.
    pub async fn skip_existing(&mut self) {
        let end = self.end_offset().await;
        self.skip_to(end);
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
        self.offset = 0;
        self.pending.clear();
        self.carry.clear();
        self.more = false;
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
    /// decoded lossily this time. Nothing is held back.
    pub async fn drain(&mut self) -> String {
        let mut out = self.read_new().await;
        // "Nothing is held back" outranks the frame cap here: this is the one
        // read whose caller has no next one to come back on.
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

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::AsyncWriteExt;

    async fn append(path: &std::path::Path, bytes: &[u8]) {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .unwrap();
        file.write_all(bytes).await.unwrap();
        file.flush().await.unwrap();
    }

    #[tokio::test]
    async fn reads_only_what_was_appended() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"hello\n").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "hello\n");
        assert_eq!(tail.read_new().await, "", "nothing new yet");

        append(&log, b"\x1b[2Jworld\r\n").await;
        assert_eq!(tail.read_new().await, "\x1b[2Jworld\r\n");
    }

    #[tokio::test]
    async fn skip_existing_starts_after_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"already rendered\n").await;

        let mut tail = LogTail::new(&log);
        tail.skip_existing().await;
        assert_eq!(tail.read_new().await, "");

        append(&log, b"new\n").await;
        assert_eq!(tail.read_new().await, "new\n");
    }

    #[tokio::test]
    async fn rewind_re_reads_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"one\ntwo\n").await;

        let mut tail = LogTail::new(&log);
        tail.skip_existing().await;
        tail.rewind();
        assert_eq!(tail.read_new().await, "one\ntwo\n");
    }

    /// pipe-pane only creates the file once the session produces output.
    #[tokio::test]
    async fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");

        let mut tail = LogTail::new(&log);
        tail.skip_existing().await;
        assert_eq!(tail.read_new().await, "");

        append(&log, b"late\n").await;
        assert_eq!(tail.read_new().await, "late\n");
    }

    #[tokio::test]
    async fn truncation_restarts_from_the_beginning() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
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
        let log = dir.path().join("console.log");
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
        let log = dir.path().join("console.log");
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

    #[tokio::test]
    async fn drain_returns_the_unread_tail_as_well() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"bye\n").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.drain().await, "bye\n");
    }

    /// A burst bigger than a frame comes back in frames, in order, whole.
    #[tokio::test]
    async fn a_large_append_is_read_a_frame_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        let burst: Vec<u8> = (0..MAX_CHUNK * 2 + 1_234)
            .map(|i| b'a' + (i % 26) as u8)
            .collect();
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        let mut read = String::new();
        let mut frames = 0;
        loop {
            let frame = tail.read_new().await;
            if frame.is_empty() {
                break;
            }
            assert!(
                frame.len() <= MAX_CHUNK,
                "no frame is bigger than the cap: {}",
                frame.len()
            );
            frames += 1;
            read.push_str(&frame);
            if !tail.has_backlog() {
                break;
            }
        }
        assert_eq!(frames, 3);
        assert_eq!(read.as_bytes(), burst.as_slice(), "in order and whole");
        assert!(!tail.has_backlog(), "and nothing left behind");
    }

    /// The cap must not fall inside a character: the frame stops short and
    /// the remainder joins the next one, exactly as a torn read does.
    #[tokio::test]
    async fn a_character_straddling_the_cap_survives() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
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
        let second = tail.read_new().await;
        assert_eq!(second, "\u{2502}tail");
    }

    /// Lossy decoding is not length-preserving: every byte that is not valid
    /// UTF-8 becomes a three-byte replacement character, so a cap on the
    /// bytes *read* would let a frame out at three times the size. The cap is
    /// on what comes back.
    #[tokio::test]
    async fn invalid_bytes_cannot_inflate_a_frame_past_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        // A cap's worth of bytes that are all invalid on their own: three
        // caps' worth of replacement characters.
        let burst = vec![0xffu8; MAX_CHUNK];
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        let mut read = String::new();
        let mut frames = 0;
        loop {
            let frame = tail.read_new().await;
            if frame.is_empty() {
                break;
            }
            assert!(
                frame.len() <= MAX_CHUNK,
                "no frame is bigger than the cap: {}",
                frame.len()
            );
            frames += 1;
            read.push_str(&frame);
            if !tail.has_backlog() {
                break;
            }
        }
        assert!(
            frames >= 3,
            "one byte read decodes to three, so a capped read is three \
             frames and a remainder: {frames}"
        );
        assert_eq!(
            read,
            String::from_utf8_lossy(&burst),
            "and nothing lost or reordered in the splitting"
        );
    }

    /// Nothing more is coming, so the cap gives way: a drain is the last read
    /// its caller will make.
    #[tokio::test]
    async fn drain_flushes_more_than_one_frame() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        let burst = vec![b'z'; MAX_CHUNK + 7];
        append(&log, &burst).await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.drain().await.len(), MAX_CHUNK + 7);
        assert_eq!(tail.drain().await, "");
    }

    #[tokio::test]
    async fn invalid_bytes_are_replaced_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"x\xffy").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "x\u{fffd}y");
    }
}
