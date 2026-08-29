//! Server-sent events, as the daemon's three streams send them.
//!
//! A response body arrives in whatever chunks the transport felt like: a frame
//! is routinely split across two of them, and two frames routinely share one.
//! So the wire is read by a parser that holds the half-line it is in the
//! middle of ([`SseParser`]) rather than by splitting each chunk on its own,
//! and the stream over it ([`crate::SseStream`]) only ever hands out frames
//! that are whole.
//!
//! The parsing rules are the HTML spec's: `CRLF`, `CR` or `LF` ends a line,
//! `field: value` with one optional space after the colon, a leading `:` is a
//! comment, a blank line dispatches what has been collected, and `data:` lines
//! are joined with newlines. What is deliberately *not* the spec's is the
//! last-event-id: [`SseEvent::id`] is the id of the frame that carried it and
//! nothing is remembered between frames, because none of the daemon's streams
//! replays — `Last-Event-ID` is documented as ignored (see the daemon's
//! `http::stream`), and a client that carried one would be claiming a
//! resumption it does not get.

/// One dispatched event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` name, or `message` where the frame named none — the
    /// spec's default, and what an `EventSource` would have dispatched it as.
    pub event: String,
    /// The `data:` lines, joined with newlines.
    pub data: String,
    /// This frame's own `id:`, where it carried one.
    pub id: Option<String>,
}

/// The default `event:` name of a frame that names none.
const DEFAULT_EVENT: &str = "message";

/// A byte-stream reader that emits whole [`SseEvent`]s.
///
/// Feed it every chunk of the body in order; it returns the frames each chunk
/// completed, and keeps whatever was left half-read for the next one.
#[derive(Debug, Default)]
pub struct SseParser {
    /// The line being read, up to the break that has not arrived yet.
    line: Vec<u8>,
    /// Set after a `CR`: an `LF` right behind it is the same line break, not
    /// the blank line that would dispatch the frame.
    after_cr: bool,
    /// Fields collected since the last dispatch.
    event: Option<String>,
    data: String,
    /// Whether any `data:` line was seen — an event whose data is genuinely
    /// empty still counts, while a frame of nothing but comments does not.
    has_data: bool,
    id: Option<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read one chunk of the body, appending everything it completed to `out`.
    pub fn push(&mut self, chunk: &[u8], out: &mut Vec<SseEvent>) {
        for &byte in chunk {
            match byte {
                b'\n' if self.after_cr => self.after_cr = false,
                b'\r' | b'\n' => {
                    self.after_cr = byte == b'\r';
                    let line = std::mem::take(&mut self.line);
                    self.line_ended(&line, out);
                }
                _ => {
                    self.after_cr = false;
                    self.line.push(byte);
                }
            }
        }
    }

    /// One complete line: a blank one dispatches, anything else is a field or
    /// a comment.
    fn line_ended(&mut self, line: &[u8], out: &mut Vec<SseEvent>) {
        if line.is_empty() {
            if let Some(event) = self.dispatch() {
                out.push(event);
            }
            return;
        }
        if line[0] == b':' {
            return; // comment: the keep-alive of the streams that use one
        }
        // Field name up to the first colon; the rest, minus one optional
        // space, is the value.
        let (name, value) = match line.iter().position(|&b| b == b':') {
            Some(colon) => {
                let rest = &line[colon + 1..];
                (&line[..colon], rest.strip_prefix(b" ").unwrap_or(rest))
            }
            None => (line, &line[..0]),
        };
        // Lossy: a frame we cannot decode is still better shown than dropped,
        // and the daemon's payloads are JSON, which is valid UTF-8 by
        // construction.
        let value = String::from_utf8_lossy(value);
        match name {
            b"event" => self.event = Some(value.into_owned()),
            b"data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(&value);
                self.has_data = true;
            }
            b"id" => self.id = Some(value.into_owned()),
            // `retry` is the reconnection delay an `EventSource` obeys;
            // nothing here reconnects on the server's schedule.
            _ => {}
        }
    }

    /// What the blank line dispatches, and the reset that follows it.
    ///
    /// A frame that carried no `data:` at all dispatches nothing — that is the
    /// spec, and it is what keeps a stray `id:` or a comment block from
    /// reaching a caller as an empty event.
    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = SseEvent {
            event: self.event.take().unwrap_or_else(|| DEFAULT_EVENT.into()),
            data: std::mem::take(&mut self.data),
            id: self.id.take(),
        };
        std::mem::take(&mut self.has_data).then_some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything in one chunk, the way a small frame usually arrives.
    fn parse(input: &str) -> Vec<SseEvent> {
        let mut out = Vec::new();
        SseParser::new().push(input.as_bytes(), &mut out);
        out
    }

    /// The same bytes fed one at a time: the parser must not care where the
    /// chunk boundaries fell.
    fn parse_by_byte(input: &str) -> Vec<SseEvent> {
        let mut parser = SseParser::new();
        let mut out = Vec::new();
        for byte in input.as_bytes() {
            parser.push(&[*byte], &mut out);
        }
        out
    }

    fn event(name: &str, data: &str, id: Option<&str>) -> SseEvent {
        SseEvent {
            event: name.into(),
            data: data.into(),
            id: id.map(str::to_owned),
        }
    }

    /// The shape the domain stream sends: an id, a name and one compact JSON
    /// payload, dispatched by the blank line.
    #[test]
    fn a_frame_is_its_id_its_name_and_its_data() {
        let wire = "id: 01M15\nevent: goal_created\ndata: {\"id\":\"01G\"}\n\n";
        assert_eq!(
            parse(wire),
            [event("goal_created", "{\"id\":\"01G\"}", Some("01M15"))]
        );
    }

    /// A body arrives in whatever chunks the transport chose, and a frame
    /// split across them — mid-field, mid-value, between the two newlines that
    /// end it — is the normal case, not the edge one.
    #[test]
    fn a_frame_split_across_chunks_is_the_same_frame() {
        let mut parser = SseParser::new();
        let mut out = Vec::new();
        for chunk in [
            "event: del",
            "ta\ndata: {\"chu",
            "nk\":\"hi\"}\n",
            "\nevent: end\ndata: {}\n\n",
        ] {
            parser.push(chunk.as_bytes(), &mut out);
        }
        assert_eq!(
            out,
            [
                event("delta", "{\"chunk\":\"hi\"}", None),
                event("end", "{}", None),
            ]
        );
    }

    /// Several frames in one chunk, which is what a busy daemon writes.
    #[test]
    fn one_chunk_can_carry_several_frames() {
        let wire = "event: a\ndata: 1\n\nevent: b\ndata: 2\n\n";
        let expected = [event("a", "1", None), event("b", "2", None)];
        assert_eq!(parse(wire), expected);
        assert_eq!(parse_by_byte(wire), expected);
    }

    /// `CRLF`, `CR` and `LF` all end a line, and a `CRLF` is one break rather
    /// than a break plus the blank line that would dispatch early.
    #[test]
    fn every_spelling_of_a_line_break_ends_one_line() {
        for wire in [
            "event: a\r\ndata: 1\r\n\r\n",
            "event: a\rdata: 1\r\r",
            "event: a\ndata: 1\n\n",
        ] {
            assert_eq!(parse(wire), [event("a", "1", None)], "{wire:?}");
            assert_eq!(parse_by_byte(wire), [event("a", "1", None)], "{wire:?}");
        }
    }

    /// One space after the colon is framing and is dropped; the rest of the
    /// value, spaces included, is data. A field with no colon at all has an
    /// empty value.
    #[test]
    fn only_the_first_space_after_the_colon_is_framing() {
        assert_eq!(parse("data:  x \n\n"), [event(DEFAULT_EVENT, " x ", None)]);
        assert_eq!(parse("data:x\n\n"), [event(DEFAULT_EVENT, "x", None)]);
        assert_eq!(parse("data\n\n"), [event(DEFAULT_EVENT, "", None)]);
    }

    /// Several `data:` lines are one payload joined by newlines — the daemon
    /// sends compact JSON precisely so it never has to, but a client that
    /// could not read it would be reading a different protocol.
    #[test]
    fn data_lines_are_joined_with_newlines() {
        assert_eq!(
            parse("data: one\ndata: two\n\n"),
            [event(DEFAULT_EVENT, "one\ntwo", None)]
        );
    }

    /// The keep-alive of the log streams is an SSE comment: it proves the
    /// connection and dispatches nothing.
    #[test]
    fn a_comment_keeps_the_connection_and_dispatches_nothing() {
        assert_eq!(parse(": keep-alive\n\n"), []);
        assert_eq!(parse("id: 01M15\n\n"), [], "no data, no event");
    }

    /// A frame the daemon has not finished writing is not an event yet: half
    /// a payload delivered early is worse than one delivered late.
    #[test]
    fn an_unterminated_frame_is_not_dispatched() {
        assert_eq!(parse("event: a\ndata: 1\n"), []);
    }

    /// Every field is reset by the dispatch, so nothing bleeds into the next
    /// frame: the second event here names no `event:` and carries no `id:`.
    #[test]
    fn the_fields_of_a_frame_do_not_outlive_it() {
        assert_eq!(
            parse("id: 1\nevent: a\ndata: x\n\ndata: y\n\n"),
            [event("a", "x", Some("1")), event(DEFAULT_EVENT, "y", None)]
        );
    }
}
