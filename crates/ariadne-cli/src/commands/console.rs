//! The interactive console for an agent session.
//!
//! An agent's trajectory is its transcript, and a line typed here becomes
//! the next `session/prompt`.
//!
//! There are two consoles behind one command. A terminal on both ends gets
//! the inline TUI in [`tui`]: a scrolling transcript with a pinned status
//! line and input box under it. Anything else — a pipe, a file, a test
//! harness — gets the plain line protocol of [`run_with_io`], one
//! `kind · summary` line for each finished item of the trajectory (040),
//! which is what a script reads.

mod tui;

use std::collections::HashMap;
use std::fmt::Display;
use std::time::Duration;

use anyhow::Result;
use chrono::TimeZone;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use ariadne_api::sessions::ConsoleInputRequest;
use ariadne_api::trajectory::{
    PermissionOptionDto, StepDto, TrajectorySnapshotDto, TrajectoryStreamQuery,
    TrajectoryUpdateDto, TurnDto,
};
use ariadne_client::{Client, SseEvent};
use ariadne_console::trajectory::{Printer, View};

use super::follow;
use super::transcript::{self, Filters, Follower, Layout};
use crate::output::{Format, note, pager};

/// Open a session's ACP console.
///
/// A terminal on both ends gets the inline TUI. Anything else gets the plain
/// line protocol, so `ariadne attach <id> < answers.txt` reads and writes
/// lines a script can follow.
pub(crate) async fn attach(client: &Client, id: &str) -> Result<()> {
    if tui::on_a_terminal() {
        return tui::attach(client, id).await;
    }
    run_with_io(
        client,
        id,
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
}

/// Show a session's turns, and follow its trajectory when asked.
pub(crate) async fn logs(
    client: &Client,
    id: &str,
    follow: bool,
    steps: bool,
    filters: Filters,
    format: Format,
) -> Result<()> {
    if !follow {
        let turns = transcript::read(client, id, &filters, steps).await?;
        return match format {
            Format::Json => {
                print!("{}", json_lines(&turns, &filters));
                Ok(())
            }
            Format::Table => pager::page(&table(&Printer::new(chrono::Local), &turns)),
        };
    }
    let layout = match format {
        Format::Json => Layout::Json,
        Format::Table => Layout::Blocks,
    };
    let mut follower = Follower::new(layout, steps, filters, Printer::new(chrono::Local));
    follow_logs(client, id, &mut follower, follow::backoff, |text| {
        print!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        Ok(())
    })
    .await
}

/// The block form of the turns a log shows.
fn table<Tz>(printer: &Printer<Tz>, turns: &[(TurnDto, Option<Vec<StepDto>>)]) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    turns
        .iter()
        .map(|(turn, steps)| printer.turn(turn, steps.as_deref()))
        .collect()
}

/// One typed turn in each line.
fn json_lines(turns: &[(TurnDto, Option<Vec<StepDto>>)], filters: &Filters) -> String {
    turns
        .iter()
        .map(|(turn, steps)| {
            let turn = transcript::json_turn(turn, steps.clone(), filters);
            Ok(format!("{}\n", serde_json::to_string(&turn)?))
        })
        .collect::<Result<String>>()
        .unwrap_or_default()
}

/// One frame of the trajectory stream.
enum Frame {
    Snapshot(Box<TrajectorySnapshotDto>),
    Update(TrajectoryUpdateDto),
    /// The follower fell behind, and the daemon closes the stream.
    Resync,
    Other,
}

fn frame(frame: &SseEvent) -> Result<Frame> {
    Ok(match frame.event.as_str() {
        "snapshot" => Frame::Snapshot(Box::new(serde_json::from_str(&frame.data)?)),
        "update" => Frame::Update(serde_json::from_str(&frame.data)?),
        "resync" => Frame::Resync,
        _ => Frame::Other,
    })
}

/// Follow the trajectory stream until the session ends or the user stops.
///
/// A dropped stream, a `resync` and an update that skips a revision each
/// reconnect, after `wait`, and the new snapshot prints only what the
/// follower has not printed. The caller decides where each piece of text
/// goes, which keeps the stream seam testable without replacing stdout.
async fn follow_logs<Tz>(
    client: &Client,
    id: &str,
    follower: &mut Follower<Tz>,
    wait: impl Fn(Option<Duration>) -> Duration,
    mut print: impl FnMut(String) -> Result<()>,
) -> Result<()>
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let query = TrajectoryStreamQuery::default();
    let mut opened_once = false;
    let mut pause = None;
    loop {
        let opened = tokio::select! {
            () = follow::interrupt() => return Ok(()),
            opened = client.trajectory_stream(id, &query) => opened,
        };
        let mut stream = match opened {
            Ok(stream) => stream,
            Err(error) if !opened_once => return Err(error.into()),
            Err(error) => {
                note(&format!("{} — reconnecting", error.human()));
                pause = Some(wait(pause));
                if !rest(pause).await {
                    return Ok(());
                }
                continue;
            }
        };
        opened_once = true;
        pause = None;
        let mut view: Option<View> = None;
        loop {
            let next = tokio::select! {
                () = follow::interrupt() => return Ok(()),
                next = stream.next() => next,
            };
            let Some(Ok(next)) = next else { break };
            match frame(&next)? {
                Frame::Snapshot(snapshot) => {
                    let fresh = View::new(*snapshot);
                    let mut stored = HashMap::new();
                    for turn_id in follower.admit(&fresh) {
                        let steps = transcript::all_steps(client, id, &turn_id).await?;
                        stored.insert(turn_id, steps);
                    }
                    print(follower.snapshot(&fresh, &stored))?;
                    if fresh.ended() {
                        return Ok(());
                    }
                    view = Some(fresh);
                }
                Frame::Update(update) => {
                    let Some(held) = view.as_mut() else { continue };
                    let mut text = String::new();
                    let applied = held.apply_each(update, |held, touched| {
                        text.push_str(&follower.update(held, std::slice::from_ref(&touched)));
                    });
                    print(text)?;
                    if applied.is_err() {
                        break;
                    }
                    if held.ended() {
                        return Ok(());
                    }
                }
                Frame::Resync => break,
                Frame::Other => {}
            }
        }
        note(&format!(
            "the trajectory stream of {id} closed — reconnecting"
        ));
        pause = Some(wait(pause));
        if !rest(pause).await {
            return Ok(());
        }
    }
}

/// Wait a pause out. False when the user interrupted it.
async fn rest(pause: Option<Duration>) -> bool {
    let pause = pause.unwrap_or_default();
    if pause.is_zero() {
        return true;
    }
    tokio::select! {
        () = follow::interrupt() => false,
        () = tokio::time::sleep(pause) => true,
    }
}

/// The console loop with supplied input and output, so its API seam can be
/// tested without taking the process terminal.
///
/// It prints one line for each finished item of the trajectory and the
/// numbered options of a permission that waits, and sends each line it
/// reads as a prompt. A number answers the waiting permission.
async fn run_with_io<R, W>(client: &Client, id: &str, input: R, mut output: W) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = input.lines();
    let mut stream = client
        .trajectory_stream(id, &TrajectoryStreamQuery::default())
        .await?;
    let mut follower = Follower::new(
        Layout::Lines,
        true,
        Filters::default(),
        Printer::new(chrono::Local),
    );
    let mut view: Option<View> = None;
    let mut options: Option<Vec<PermissionOptionDto>> = None;

    // The stream opens with its snapshot. Print it before accepting a line,
    // so a numeric answer has the permission choices it names even when
    // stdin already has data waiting.
    let Some(first) = stream.next().await else {
        return Ok(());
    };
    apply(&mut view, &mut follower, &frame(&first?)?, &mut output).await?;
    options = waiting(&view).or(options);

    loop {
        tokio::select! {
            next = stream.next() => match next {
                Some(next) => {
                    apply(&mut view, &mut follower, &frame(&next?)?, &mut output).await?;
                    options = waiting(&view).or(options);
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

/// Take one frame into the view, and write what it adds.
async fn apply<Tz, W>(
    view: &mut Option<View>,
    follower: &mut Follower<Tz>,
    frame: &Frame,
    output: &mut W,
) -> Result<()>
where
    Tz: TimeZone,
    Tz::Offset: Display,
    W: AsyncWrite + Unpin,
{
    let text = match frame {
        Frame::Snapshot(snapshot) => {
            let fresh = View::new(snapshot.as_ref().clone());
            follower.admit(&fresh);
            let text = follower.snapshot(&fresh, &HashMap::new());
            *view = Some(fresh);
            text
        }
        Frame::Update(update) => match view.as_mut() {
            Some(held) => {
                let mut text = String::new();
                held.apply_each(update.clone(), |held, touched| {
                    text.push_str(&follower.update(held, std::slice::from_ref(&touched)));
                })
                .map_err(|gap| {
                    anyhow::anyhow!("the trajectory stream skipped a revision: {gap}")
                })?;
                text
            }
            None => String::new(),
        },
        Frame::Resync => anyhow::bail!("the console fell behind the session — attach again"),
        Frame::Other => String::new(),
    };
    output.write_all(text.as_bytes()).await?;
    output.flush().await?;
    Ok(())
}

/// The options of the newest permission that waits.
fn waiting(view: &Option<View>) -> Option<Vec<PermissionOptionDto>> {
    view.as_ref()?
        .waiting_permission()
        .map(|permission| permission.options.clone())
}

/// A permission question accepts its displayed number. The selected option
/// identifier becomes the next prompt, just as text entered at the console.
fn selected_option(line: &str, options: Option<Vec<PermissionOptionDto>>) -> Option<String> {
    let index = line.trim().parse::<usize>().ok()?.checked_sub(1)?;
    options?.get(index).map(|option| option.id.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::convert::Infallible;
    use std::sync::{Arc, Mutex};

    use axum::extract::{Path, Query, State};
    use axum::http::StatusCode;
    use axum::response::sse::{Event, Sse};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use chrono::Utc;
    use futures_util::{StreamExt, stream};
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, duplex};

    use ariadne_api::trajectory::{TrajectoryQuery, TrajectoryStepsQuery};

    use super::*;

    /// A daemon that answers the trajectory routes from fixed JSON, and
    /// opens each stream on the next script of frames.
    /// The frames of one stream connection: each event name and its data.
    type Script = Vec<(&'static str, Value)>;

    #[derive(Clone, Default)]
    struct Stub {
        /// The newest page, and each older page by the turn it comes before.
        pages: HashMap<String, Value>,
        /// The steps of each turn.
        steps: HashMap<String, Value>,
        streams: Arc<Mutex<VecDeque<Script>>>,
        prompts: Arc<Mutex<Vec<String>>>,
        /// The `before` of each page read, `-` for the newest.
        reads: Arc<Mutex<Vec<String>>>,
    }

    async fn page(State(stub): State<Stub>, Query(query): Query<TrajectoryQuery>) -> Json<Value> {
        let key = query.before.unwrap_or_else(|| "-".into());
        stub.reads.lock().unwrap().push(key.clone());
        Json(stub.pages[&key].clone())
    }

    async fn steps(
        State(stub): State<Stub>,
        Path((_, turn_id)): Path<(String, String)>,
        Query(_): Query<TrajectoryStepsQuery>,
    ) -> Json<Value> {
        Json(
            json!({"steps": stub.steps.get(&turn_id).cloned().unwrap_or(json!([])),
                    "earlier_steps": 0}),
        )
    }

    async fn trajectory_stream(
        State(stub): State<Stub>,
    ) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
        let script = stub.streams.lock().unwrap().pop_front();
        // A connection past the last script holds still, as a quiet session.
        let open = script.is_none();
        let frames = script.unwrap_or_default().into_iter().map(|(name, data)| {
            Ok(Event::default()
                .event(name)
                .data(serde_json::to_string(&data).unwrap()))
        });
        Sse::new(stream::iter(frames).chain(stream::pending().take(if open {
            usize::MAX
        } else {
            0
        })))
    }

    async fn post_input(
        State(stub): State<Stub>,
        Json(input): Json<ConsoleInputRequest>,
    ) -> StatusCode {
        stub.prompts.lock().unwrap().push(input.text);
        StatusCode::NO_CONTENT
    }

    async fn serve(stub: Stub) -> (Client, tokio::task::JoinHandle<()>) {
        let app = Router::new()
            .route("/v1/sessions/{id}/trajectory", get(page))
            .route(
                "/v1/sessions/{id}/trajectory/stream",
                get(trajectory_stream),
            )
            .route(
                "/v1/sessions/{id}/trajectory/turns/{turn_id}/steps",
                get(steps),
            )
            .route("/v1/sessions/{id}/console/input", post(post_input))
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::tcp(format!("http://{address}")), server)
    }

    fn turn(id: &str, state: &str, extra: Value) -> Value {
        let mut turn = json!({
            "id": id, "lane": "main",
            "prompt": {"id": id, "source": "console", "prompt_kind": null,
                       "content": [{"type": "text", "text": format!("prompt {id}")}]},
            "state": state, "stop_reason": null, "started_at": "2026-11-27T10:00:00Z",
            "ended_at": if state == "running" { Value::Null } else { json!("2026-11-27T10:05:00Z") },
            "activity": {"steps": 2, "tool_calls": 2, "files_changed": 0, "failed": 0,
                         "cancelled": 0, "thoughts": 0, "subagents": 0},
            "steps": [], "earlier_steps": 0, "attention": [], "attention_more": 0,
            "answer": [{"type": "text", "text": format!("answer {id}")}]
        });
        turn.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        turn
    }

    fn call(id: &str, turn_id: &str, status: &str) -> Value {
        json!({"type": "tool_call", "id": id, "turn_id": turn_id, "lane": "main",
               "created_at": "2026-11-27T10:01:00Z", "tool_call_id": id, "title": format!("call {id}"),
               "kind": "execute", "status": status, "subject": null, "locations": [],
               "preview": null, "diff_stat": null, "started_at": "2026-11-27T10:01:00Z",
               "ended_at": null, "permission": null, "detail_rev": 0})
    }

    fn permission(id: &str, turn_id: &str) -> Value {
        json!({"type": "permission", "id": id, "turn_id": turn_id, "lane": "main",
               "created_at": "2026-11-27T10:02:00Z", "tool_call_id": "c1",
               "tool": {"title": "Write", "kind": "edit", "subject": "src/main.rs"},
               "question": "Allow Write src/main.rs?", "state": "pending", "answer": null,
               "ai": null, "learned": null, "subagent": null,
               "options": [{"id": "no", "name": "Reject", "kind": "reject_once"},
                           {"id": "yes", "name": "Allow", "kind": "allow_once"}]})
    }

    fn snapshot(rev: u64, status: &str, turns: Vec<Value>, has_older: bool) -> Value {
        json!({
            "rev": rev,
            "session": session(status),
            "turns": turns, "has_older": has_older, "subagents": [], "plans": []
        })
    }

    fn session(status: &str) -> Value {
        json!({"status": status, "attention_reason": null, "running_turn_id": null,
               "title": null, "mode": null, "config_options": [], "context": null,
               "usage": {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0},
               "cost": null, "commands": []})
    }

    #[tokio::test]
    async fn a_log_prints_each_turn_folded_and_with_steps_each_step_under_it() {
        let stub = Stub {
            pages: HashMap::from([(
                "-".into(),
                snapshot(0, "idle", vec![turn("t1", "completed", json!({}))], false),
            )]),
            steps: HashMap::from([(
                "t1".into(),
                json!([call("c1", "t1", "completed"), call("c2", "t1", "failed")]),
            )]),
            ..Stub::default()
        };
        let (client, server) = serve(stub).await;
        let printer = Printer::new(Utc);

        let folded = transcript::read(&client, "s", &Filters::default(), false)
            .await
            .unwrap();
        let whole = transcript::read(&client, "s", &Filters::default(), true)
            .await
            .unwrap();

        assert_eq!(
            table(&printer, &folded),
            "10:00:00  USER  prompt t1\n\
             \x20 completed · 2 steps · 2 tool calls\n\
             10:05:00  AGENT  answer t1\n\n"
        );
        assert_eq!(
            table(&printer, &whole),
            "10:00:00  USER  prompt t1\n\
             \x20 10:01:00  tool_call  call c1 · completed\n\
             \x20 10:01:00  tool_call  call c2 · failed\n\
             \x20 completed · 2 steps · 2 tool calls\n\
             10:05:00  AGENT  answer t1\n\n"
        );
        let kinds = Filters {
            kinds: vec!["error".into()],
            ..Filters::default()
        };
        let none = transcript::read(&client, "s", &kinds, true).await.unwrap();
        assert_eq!(none[0].1.as_deref(), Some(&[][..]), "no step is an error");
        server.abort();
    }

    #[tokio::test]
    async fn tail_reads_older_pages_until_it_has_its_turns() {
        let stub = Stub {
            pages: HashMap::from([
                (
                    "-".into(),
                    snapshot(0, "idle", vec![turn("t3", "completed", json!({}))], true),
                ),
                (
                    "t3".into(),
                    snapshot(
                        0,
                        "idle",
                        vec![
                            turn("t1", "completed", json!({})),
                            turn("t2", "completed", json!({})),
                        ],
                        false,
                    ),
                ),
            ]),
            ..Stub::default()
        };
        let reads = stub.reads.clone();
        let (client, server) = serve(stub).await;
        let filters = Filters {
            tail: Some(2),
            ..Filters::default()
        };

        let turns = transcript::read(&client, "s", &filters, false)
            .await
            .unwrap();

        let ids: Vec<_> = turns.iter().map(|(turn, _)| turn.id.as_str()).collect();
        assert_eq!(ids, ["t2", "t3"]);
        assert_eq!(*reads.lock().unwrap(), ["-", "t3"]);
        server.abort();
    }

    #[tokio::test]
    async fn json_prints_one_typed_turn_in_each_line() {
        let stub = Stub {
            pages: HashMap::from([(
                "-".into(),
                snapshot(
                    0,
                    "idle",
                    vec![
                        turn("t1", "completed", json!({})),
                        turn(
                            "t2",
                            "running",
                            json!({"steps": [call("c1", "t2", "running")]}),
                        ),
                    ],
                    false,
                ),
            )]),
            ..Stub::default()
        };
        let (client, server) = serve(stub).await;

        let turns = transcript::read(&client, "s", &Filters::default(), false)
            .await
            .unwrap();
        let out = json_lines(&turns, &Filters::default());

        let lines: Vec<TurnDto> = out
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert_eq!(lines[1].id, "t2");
        assert!(lines[1].steps.is_empty());
        assert_eq!(
            lines[1].earlier_steps, 1,
            "the step it leaves out is counted"
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_follow_prints_each_finished_item_once_also_after_a_reconnect() {
        let running = |steps: Vec<Value>| turn("t1", "running", json!({"steps": steps}));
        let streams = VecDeque::from([
            vec![
                (
                    "snapshot",
                    snapshot(
                        3,
                        "running",
                        vec![running(vec![call("c1", "t1", "completed")])],
                        false,
                    ),
                ),
                (
                    "update",
                    json!({"rev": 4, "ops": [
                        {"op": "item_upsert", "step": call("c2", "t1", "running")},
                    ]}),
                ),
            ],
            vec![
                (
                    "snapshot",
                    snapshot(
                        9,
                        "running",
                        vec![running(vec![
                            call("c1", "t1", "completed"),
                            call("c2", "t1", "completed"),
                        ])],
                        false,
                    ),
                ),
                (
                    "update",
                    json!({"rev": 10, "ops": [
                        {"op": "item_upsert", "step": call("c3", "t1", "completed")},
                        {"op": "turn_upsert", "turn": turn("t1", "completed", json!({}))},
                        {"op": "session_patch", "session": session("exited")},
                    ]}),
                ),
            ],
        ]);
        let stub = Stub {
            streams: Arc::new(Mutex::new(streams)),
            ..Stub::default()
        };
        let (client, server) = serve(stub).await;
        let mut follower =
            Follower::new(Layout::Lines, false, Filters::default(), Printer::new(Utc));
        let mut out = String::new();

        follow_logs(
            &client,
            "s",
            &mut follower,
            |_| Duration::ZERO,
            |text| {
                out.push_str(&text);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(
            out,
            "prompt · prompt t1\n\
             tool_call · call c1 · completed\n\
             tool_call · call c2 · completed\n\
             tool_call · call c3 · completed\n\
             turn · completed · 2 steps · 2 tool calls\n"
        );
        server.abort();
    }

    async fn console(stub: Stub, line: &str) -> (String, Vec<String>) {
        let prompts = stub.prompts.clone();
        let (client, server) = serve(stub).await;
        let (mut input_writer, input_reader) = duplex(128);
        input_writer.write_all(line.as_bytes()).await.unwrap();
        input_writer.shutdown().await.unwrap();
        let (output_writer, mut output_reader) = duplex(4096);
        run_with_io(&client, "s", BufReader::new(input_reader), output_writer)
            .await
            .unwrap();
        let mut output = String::new();
        output_reader.read_to_string(&mut output).await.unwrap();
        server.abort();
        (output, prompts.lock().unwrap().clone())
    }

    fn opening(turns: Vec<Value>) -> Stub {
        Stub {
            streams: Arc::new(Mutex::new(VecDeque::from([vec![(
                "snapshot",
                snapshot(0, "running", turns, false),
            )]]))),
            ..Stub::default()
        }
    }

    #[tokio::test]
    async fn a_piped_console_prints_a_line_for_each_finished_item_and_sends_a_line_as_a_prompt() {
        let stub = opening(vec![
            turn("t1", "completed", json!({})),
            turn(
                "t2",
                "running",
                json!({"steps": [call("c1", "t2", "completed"), call("c2", "t2", "running")]}),
            ),
        ]);

        let (output, prompts) = console(stub, "continue\n").await;

        assert_eq!(
            output,
            "prompt · prompt t1\n\
             turn · completed · 2 steps · 2 tool calls\n\
             answer · answer t1\n\
             prompt · prompt t2\n\
             tool_call · call c1 · completed\n"
        );
        assert_eq!(prompts, ["continue"]);
    }

    #[tokio::test]
    async fn a_piped_console_numbers_a_permission_and_sends_the_option_chosen() {
        let stub = opening(vec![turn(
            "t1",
            "running",
            json!({"steps": [permission("p1", "t1")], "attention": [permission("p1", "t1")]}),
        )]);

        let (output, prompts) = console(stub, "2\n").await;

        assert!(
            output.ends_with("permission · Allow Write src/main.rs?\n  1. Reject\n  2. Allow\n"),
            "{output}"
        );
        assert_eq!(prompts, ["yes"]);
    }
}
