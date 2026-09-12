//! The interactive console for an agent session.
//!
//! An agent's recorded events are its transcript, and a line typed here
//! becomes the next `session/prompt`.

use anyhow::Result;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use ariadne_api::events::AgentEventDto;
use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_client::{Client, SseEvent};

use super::follow::{self, Ending, Next};
use super::transcript::{self, Filters};
use crate::output::{Format, note, pager, print_json, view};

/// Open a session's ACP console, rendering its recorded events and accepting
/// one prompt on each input line.
pub async fn attach(client: &Client, id: &str) -> Result<()> {
    run_with_io(
        client,
        id,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
}

/// Show an ACP session's transcript, and follow its event stream when asked.
pub async fn logs(
    client: &Client,
    id: &str,
    follow: bool,
    filters: Filters,
    format: Format,
) -> Result<()> {
    if !follow {
        let events = snapshot(client, id).await?;
        return match format {
            Format::Json => print_json(&filters.apply_events(&events)),
            Format::Table => {
                let items = filters.apply(transcript::fold(&events));
                pager::page(&transcript::render::transcript(
                    &items,
                    view().width,
                    view().color,
                ))
            }
        };
    }

    let ending = follow_logs(client, id, format, &filters, |text| {
        print!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        Ok(())
    })
    .await?;
    if ending == Ending::Dropped {
        note(&format!(
            "console stream for {id} closed while the session was still live — \
             run it again to reconnect"
        ));
    }
    Ok(())
}

/// Read the recorded ACP transcript.
async fn snapshot(client: &Client, id: &str) -> Result<Vec<AgentEventDto>> {
    client
        .get_json(&format!("/v1/sessions/{id}/console"))
        .await
        .map_err(Into::into)
}

/// Follow the ACP event stream. The caller decides where each rendered event
/// goes, which keeps the stream seam testable without replacing stdout.
async fn follow_logs(
    client: &Client,
    id: &str,
    format: Format,
    filters: &Filters,
    mut print: impl FnMut(String) -> Result<()>,
) -> Result<Ending> {
    let mut renderer = transcript::render::StreamRenderer::new(view().width, view().color);
    let ending = follow::frames(
        client,
        &format!("/v1/sessions/{id}/console/stream"),
        |frame| {
            let events = events(&frame)?;
            let session_ended = events.iter().any(|event| event.kind == "session_end");
            match (format, frame.event.as_str()) {
                (Format::Json, "snapshot") => {
                    for event in filters.apply_events(&events) {
                        print(format!("{}\n", serde_json::to_string(event)?))?;
                    }
                }
                (Format::Json, "event") => {
                    for event in events {
                        if filters.allows_kind(&event.kind) {
                            print(format!("{}\n", serde_json::to_string(&event)?))?;
                        }
                    }
                }
                (Format::Table, "snapshot") => print(renderer.snapshot(&events, filters))?,
                (Format::Table, "event") => {
                    for event in events {
                        print(renderer.event(&event, filters))?;
                    }
                }
                _ => {}
            }
            Ok(if session_ended { Next::Stop } else { Next::Go })
        },
    )
    .await?;
    print(renderer.finish())?;
    Ok(ending)
}

/// The console loop with supplied input and output, so its API seam can be
/// tested without taking the process terminal.
async fn run_with_io<R, W>(client: &Client, id: &str, input: R, mut output: W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = input.lines();
    let mut stream = client
        .stream(&format!("/v1/sessions/{id}/console/stream"))
        .await?;
    let mut options: Option<Vec<PermissionOption>> = None;

    // The console stream always opens with its transcript snapshot. Render it
    // before accepting a line, so a numeric answer has the permission choices
    // it names even when stdin already has data waiting.
    let Some(frame) = stream.next().await else {
        return Ok(());
    };
    for event in events(&frame?)? {
        options = render_to(&mut output, &event).await?.or(options);
    }

    loop {
        tokio::select! {
            frame = stream.next() => match frame {
                Some(frame) => {
                    let frame = frame?;
                    for event in events(&frame)? {
                        options = render_to(&mut output, &event).await?.or(options);
                    }
                }
                None => return Ok(()),
            },
            line = lines.next_line() => match line? {
                Some(line) => {
                    let text = selected_option(&line, options.take()).unwrap_or(line);
                    client
                        .send_no_content(
                            http::Method::POST,
                            &format!("/v1/sessions/{id}/console/input"),
                            Some(&ConsoleInputRequest { text }),
                        )
                        .await?;
                }
                None => return Ok(()),
            },
            _ = tokio::signal::ctrl_c() => return Ok(()),
        }
    }
}

/// Events carried by one console stream frame.
fn events(frame: &SseEvent) -> Result<Vec<AgentEventDto>> {
    match frame.event.as_str() {
        "snapshot" => Ok(serde_json::from_str(&frame.data)?),
        "event" => Ok(vec![serde_json::from_str(&frame.data)?]),
        _ => Ok(Vec::new()),
    }
}

/// One event in the compact transcript form used by the console and logs.
fn render(event: &AgentEventDto) -> String {
    format!("{} · {}", event.kind, event.summary)
}

/// Write one event and return options from a permission question.
async fn render_to<W: AsyncWrite + Unpin>(
    output: &mut W,
    event: &AgentEventDto,
) -> Result<Option<Vec<PermissionOption>>> {
    output.write_all(render(event).as_bytes()).await?;
    output.write_all(b"\n").await?;
    let options = permission_options(event);
    if let Some(options) = &options {
        for (index, option) in options.iter().enumerate() {
            output
                .write_all(format!("  {}. {}\n", index + 1, option.name).as_bytes())
                .await?;
        }
    }
    output.flush().await?;
    Ok(options)
}

/// The option identifiers a permission question offers, in display order.
#[derive(Debug, Clone)]
struct PermissionOption {
    id: String,
    name: String,
}

fn permission_options(event: &AgentEventDto) -> Option<Vec<PermissionOption>> {
    (event.kind == "permission_request").then(|| {
        event
            .payload
            .get("options")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|option| {
                let id = option.get("optionId")?.as_str()?.to_string();
                let name = option
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                Some(PermissionOption { id, name })
            })
            .collect()
    })
}

/// A permission question accepts its displayed number. The selected option
/// identifier becomes the next prompt, just as text entered at the console.
fn selected_option(line: &str, options: Option<Vec<PermissionOption>>) -> Option<String> {
    let index = line.trim().parse::<usize>().ok()?.checked_sub(1)?;
    options?.get(index).map(|option| option.id.clone())
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::sse::{Event, Sse};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use futures_util::{StreamExt, stream};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

    use super::*;

    #[derive(Clone)]
    struct StubAgent {
        events: Vec<AgentEventDto>,
        stream_events: Vec<AgentEventDto>,
        prompts: Arc<Mutex<Vec<String>>>,
        keep_stream_open: bool,
    }

    async fn stream(
        State(agent): State<StubAgent>,
    ) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
        let snapshot = Event::default()
            .event("snapshot")
            .data(serde_json::to_string(&agent.events).unwrap());
        let deltas = agent.stream_events.into_iter().map(|event| {
            Ok(Event::default()
                .event("event")
                .data(serde_json::to_string(&event).unwrap()))
        });
        let remaining = if agent.keep_stream_open {
            usize::MAX
        } else {
            0
        };
        Sse::new(
            stream::once(async move { Ok(snapshot) })
                .chain(stream::iter(deltas))
                .chain(stream::pending().take(remaining)),
        )
    }

    async fn console_snapshot(State(agent): State<StubAgent>) -> Json<Vec<AgentEventDto>> {
        Json(agent.events)
    }

    async fn post_input(
        State(agent): State<StubAgent>,
        Json(input): Json<ConsoleInputRequest>,
    ) -> StatusCode {
        agent.prompts.lock().unwrap().push(input.text);
        StatusCode::NO_CONTENT
    }

    async fn api(
        events: Vec<AgentEventDto>,
        keep_stream_open: bool,
    ) -> (Client, tokio::task::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
        api_with_stream(events, Vec::new(), keep_stream_open).await
    }

    async fn api_with_stream(
        events: Vec<AgentEventDto>,
        stream_events: Vec<AgentEventDto>,
        keep_stream_open: bool,
    ) -> (Client, tokio::task::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let agent = StubAgent {
            events,
            stream_events,
            prompts: prompts.clone(),
            keep_stream_open,
        };
        let app = Router::new()
            .route("/v1/sessions/session/console", get(console_snapshot))
            .route("/v1/sessions/session/console/stream", get(stream))
            .route("/v1/sessions/session/console/input", post(post_input))
            .with_state(agent);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), server, prompts)
    }

    async fn console(events: Vec<AgentEventDto>, line: &str) -> (String, Vec<String>) {
        let (client, server, prompts) = api(events, true).await;
        let (mut input_writer, input_reader) = duplex(128);
        input_writer.write_all(line.as_bytes()).await.unwrap();
        input_writer.shutdown().await.unwrap();
        let (output_writer, mut output_reader) = duplex(1024);
        run_with_io(
            &client,
            "session",
            BufReader::new(input_reader),
            output_writer,
        )
        .await
        .unwrap();
        let mut output = String::new();
        output_reader.read_to_string(&mut output).await.unwrap();
        server.abort();
        (output, prompts.lock().unwrap().clone())
    }

    fn event(kind: &str, summary: &str, payload: serde_json::Value) -> AgentEventDto {
        event_at(kind, summary, payload, "2026-09-11T00:00:00Z")
    }

    fn event_at(
        kind: &str,
        summary: &str,
        payload: serde_json::Value,
        created_at: &str,
    ) -> AgentEventDto {
        AgentEventDto {
            id: format!("event-{kind}-{created_at}"),
            session_id: Some("session".into()),
            task_id: None,
            kind: kind.into(),
            payload,
            summary: summary.into(),
            created_at: created_at.into(),
        }
    }

    #[tokio::test]
    async fn a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line() {
        let (output, prompts) = console(
            vec![event("agent_message", "Stub agent is ready", json!({}))],
            "continue\n",
        )
        .await;

        assert!(output.contains("Stub agent is ready"), "{output}");
        assert_eq!(prompts, ["continue"]);
    }

    #[tokio::test]
    async fn a_permission_question_renders_and_delivers_the_selected_answer() {
        let (output, prompts) = console(
            vec![event(
                "permission_request",
                "Permission requested for Write",
                json!({"options": [
                    {"optionId": "no", "name": "Reject"},
                    {"optionId": "yes", "name": "Allow"}
                ]}),
            )],
            "2\n",
        )
        .await;

        assert!(
            output.contains("Permission requested for Write"),
            "{output}"
        );
        assert!(output.contains("2. Allow"), "{output}");
        assert_eq!(prompts, ["yes"]);
    }

    #[tokio::test]
    async fn a_transcript_log_uses_its_snapshot_for_table_and_json_output() {
        let event = event("stop", "Stub agent finished", json!({}));
        let (client, server, _) = api(vec![event.clone()], false).await;

        let events = snapshot(&client, "session").await.unwrap();
        let output = transcript::render::transcript(&transcript::fold(&events), None, false);
        assert!(output.contains("turn stopped"), "{output}");
        assert_eq!(
            serde_json::to_value(Filters::default().apply_events(&events)).unwrap(),
            serde_json::to_value(&events).unwrap(),
            "JSON keeps each daemon event unchanged"
        );
        server.abort();
    }

    #[tokio::test]
    async fn transcript_snapshot_filters_apply_to_folded_items() {
        let events = vec![
            event_at(
                "agent_message",
                "old",
                json!({"text": "old message"}),
                "2026-09-11T10:00:00Z",
            ),
            event_at(
                "user_prompt_submit",
                "prompt",
                json!({"text": "new prompt"}),
                "2026-09-11T11:00:00Z",
            ),
            event_at(
                "agent_message",
                "new",
                json!({"text": "new message"}),
                "2026-09-11T12:00:00Z",
            ),
        ];
        let (client, server, _) = api(events, false).await;
        let events = snapshot(&client, "session").await.unwrap();

        let tail = Filters {
            tail: Some(2),
            ..Filters::default()
        }
        .apply(transcript::fold(&events));
        assert_eq!(tail.len(), 2);
        let tail = transcript::render::transcript(&tail, None, false);
        assert!(!tail.contains("old message"), "{tail}");
        assert!(
            tail.contains("new prompt") && tail.contains("new message"),
            "{tail}"
        );

        let since = Filters {
            since: Some("2026-09-11T10:30:00Z".parse().unwrap()),
            ..Filters::default()
        }
        .apply(transcript::fold(&events));
        assert_eq!(since.len(), 2);
        let since = transcript::render::transcript(&since, None, false);
        assert!(!since.contains("old message"), "{since}");

        let messages = Filters {
            kinds: vec!["agent_message".into()],
            ..Filters::default()
        }
        .apply(transcript::fold(&events));
        assert_eq!(messages.len(), 2);
        let messages = transcript::render::transcript(&messages, None, false);
        assert!(!messages.contains("new prompt"), "{messages}");
        assert!(messages.contains("old message") && messages.contains("new message"));
        server.abort();
    }

    #[tokio::test]
    async fn a_followed_log_uses_the_console_event_stream() {
        let event = event("stop", "Stub agent finished", json!({}));
        let (client, server, _) = api(vec![event.clone()], false).await;
        let mut output = String::new();

        let ending = follow_logs(
            &client,
            "session",
            Format::Json,
            &Filters::default(),
            |text| {
                output.push_str(&text);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(ending, Ending::Dropped);
        assert_eq!(
            serde_json::from_str::<AgentEventDto>(output.lines().next().unwrap())
                .unwrap()
                .kind,
            "stop"
        );
        server.abort();
    }

    #[tokio::test]
    async fn followed_chunks_stream_text_under_one_block_header() {
        let snapshot = vec![event(
            "agent_message_chunk",
            "Half ",
            json!({"text": "Half "}),
        )];
        let deltas = vec![
            event(
                "agent_thought_chunk",
                "hidden thought",
                json!({"text": "hidden thought"}),
            ),
            event("agent_message_chunk", "done.", json!({"text": "done."})),
            event("agent_message", "Half done.", json!({"text": "Half done."})),
        ];
        let (client, server, _) = api_with_stream(snapshot, deltas, false).await;
        let mut output = String::new();
        let filters = Filters {
            kinds: vec!["agent_message_chunk".into()],
            ..Filters::default()
        };

        follow_logs(&client, "session", Format::Table, &filters, |text| {
            output.push_str(&text);
            Ok(())
        })
        .await
        .unwrap();

        assert!(output.contains("Half done."), "{output:?}");
        assert!(!output.contains("hidden thought"), "{output:?}");
        assert_eq!(output.matches("AGENT").count(), 1, "{output:?}");
        server.abort();
    }
}
