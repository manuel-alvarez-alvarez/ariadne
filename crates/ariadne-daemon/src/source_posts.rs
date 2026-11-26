//! The open event streams of source daemons, on a worker (039).
//!
//! A worker has no call to its source: a post of a worker agent to the source
//! orchestrator goes out as a `source_post` frame on an event stream that the
//! source holds open here. Nothing holds a post for a later stream, so a
//! reconnect never delivers one again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ariadne_api::messages::SourcePostDto;
use tokio::sync::mpsc;

type Streams = HashMap<String, Vec<(u64, mpsc::UnboundedSender<SourcePostDto>)>>;

/// Each open stream of each source, by its daemon ID. Cheap to clone: every
/// clone is the same.
#[derive(Clone, Default)]
pub struct SourcePosts {
    streams: Arc<Mutex<Streams>>,
    next: Arc<AtomicU64>,
}

/// One open stream of a source. Its drop closes it.
pub(crate) struct OpenStream {
    posts: SourcePosts,
    source: String,
    id: u64,
}

impl Drop for OpenStream {
    fn drop(&mut self) {
        let mut streams = self.posts.streams.lock().unwrap();
        if let Some(open) = streams.get_mut(&self.source) {
            open.retain(|(id, _)| *id != self.id);
            if open.is_empty() {
                streams.remove(&self.source);
            }
        }
    }
}

impl SourcePosts {
    /// Open a stream of `source`: the posts for it arrive on the receiver
    /// while the [`OpenStream`] lives.
    pub(crate) fn open(
        &self,
        source: &str,
    ) -> (OpenStream, mpsc::UnboundedReceiver<SourcePostDto>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.streams
            .lock()
            .unwrap()
            .entry(source.to_string())
            .or_default()
            .push((id, tx));
        let open = OpenStream {
            posts: self.clone(),
            source: source.to_string(),
            id,
        };
        (open, rx)
    }

    /// Whether `source` holds a stream open here.
    pub(crate) fn is_open(&self, source: &str) -> bool {
        self.streams
            .lock()
            .unwrap()
            .get(source)
            .is_some_and(|open| open.iter().any(|(_, tx)| !tx.is_closed()))
    }

    /// Hand `post` to the newest open stream of `source`, and to that one
    /// alone, so the source gets it one time at most. `false` where no stream
    /// of the source is open.
    pub(crate) fn send(&self, source: &str, post: SourcePostDto) -> bool {
        let streams = self.streams.lock().unwrap();
        let Some(open) = streams.get(source) else {
            return false;
        };
        let mut post = post;
        for (_, tx) in open.iter().rev() {
            match tx.send(post) {
                Ok(()) => return true,
                Err(mpsc::error::SendError(back)) => post = back,
            }
        }
        false
    }
}
