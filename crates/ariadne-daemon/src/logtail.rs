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

/// A cursor into one console log.
pub struct LogTail {
    path: PathBuf,
    offset: u64,
    /// Trailing bytes of an incomplete UTF-8 character: a read can land in the
    /// middle of one, and half a character is worth carrying rather than
    /// decoding into a replacement glyph.
    pending: Vec<u8>,
}

impl LogTail {
    /// A tail positioned at the start of `path`. The file need not exist yet.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            pending: Vec::new(),
        }
    }

    /// Skip whatever the file already holds — for when the caller has just
    /// sent a snapshot covering it.
    pub async fn skip_existing(&mut self) {
        self.offset = tokio::fs::metadata(&self.path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        self.pending.clear();
    }

    /// Position back at the start of the file.
    pub fn rewind(&mut self) {
        self.offset = 0;
        self.pending.clear();
    }

    /// Everything appended since the last read, decoded lossily. Empty when
    /// the file is absent, unchanged, or unreadable.
    pub async fn read_new(&mut self) -> String {
        let Ok(mut file) = tokio::fs::File::open(&self.path).await else {
            // Not there (yet): pipe-pane creates it as the session starts.
            return String::new();
        };
        let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Truncated or replaced under us: re-read rather than sit past EOF.
            self.rewind();
        }
        if len == self.offset {
            return String::new();
        }
        if let Err(e) = file.seek(SeekFrom::Start(self.offset)).await {
            warn!(path = %self.path.display(), error = %e, "seeking console log failed");
            return String::new();
        }
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        match file.read_to_end(&mut buf).await {
            Ok(n) => self.offset += n as u64,
            Err(e) => {
                warn!(path = %self.path.display(), error = %e, "reading console log failed");
                return String::new();
            }
        }
        self.decode(buf)
    }

    /// Everything that is left, for when nothing more will ever be appended:
    /// the last unread bytes plus any half-written character carried from an
    /// earlier read, this time decoded lossily. Nothing is held back.
    pub async fn drain(&mut self) -> String {
        let mut out = self.read_new().await;
        if !self.pending.is_empty() {
            let leftover = std::mem::take(&mut self.pending);
            out.push_str(&String::from_utf8_lossy(&leftover));
        }
        out
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

    #[tokio::test]
    async fn invalid_bytes_are_replaced_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("console.log");
        append(&log, b"x\xffy").await;

        let mut tail = LogTail::new(&log);
        assert_eq!(tail.read_new().await, "x\u{fffd}y");
    }
}
