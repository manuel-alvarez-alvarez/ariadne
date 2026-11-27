//! The interactive console for an agent session.
//!
//! An agent's trajectory is its transcript, and a line typed here becomes
//! the next `session/prompt`.
//!
//! There are two consoles behind one command. A terminal on both ends gets
//! the full-screen console in [`app`]: the trajectory (040) as turns on the
//! whole screen, with the input box under it. Anything else — a pipe, a
//! file, a test harness — gets the plain line protocol of [`run_with_io`],
//! one `kind · summary` line for each finished item of the trajectory, which
//! is what a script reads.

mod app;
mod terminal;

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
/// A terminal on both ends gets the full-screen console. Anything else gets
/// the plain line protocol, so `ariadne attach <id> < answers.txt` reads and
/// writes lines a script can follow.
pub(crate) async fn attach(client: &Client, id: &str) -> Result<()> {
    if terminal::on_a_terminal() {
        return app::attach(client, id).await;
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
    let query = TrajectoryStreamQuery {
        limit: Some(transcript::TURN_PAGE),
    };
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
                    // The turns the snapshot does not hold: those `--tail`
                    // and `--since` reach at the start, and those that came
                    // and went while the stream was down.
                    let has_older = snapshot.has_older;
                    let fresh = View::new(*snapshot);
                    let older = transcript::older(client, id, fresh.turns(), has_older, |turns| {
                        follower.wants_older(turns)
                    })
                    .await?;
                    let mut stored = HashMap::new();
                    for turn_id in follower.admit(&older, &fresh) {
                        let steps = transcript::all_steps(client, id, &turn_id).await?;
                        stored.insert(turn_id, steps);
                    }
                    print(follower.snapshot(&older, &fresh, &stored))?;
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
/// reads as a prompt. A number answers the permission that waits when the
/// line is read, once; any other line, or a number when none waits, is a
/// prompt.
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
    // The permission a number already answered: a second number is a prompt.
    let mut answered: Option<String> = None;

    // The stream opens with its snapshot. Print it before accepting a line,
    // so a numeric answer has the permission choices it names even when
    // stdin already has data waiting.
    let Some(first) = stream.next().await else {
        return Ok(());
    };
    apply(&mut view, &mut follower, &frame(&first?)?, &mut output).await?;

    loop {
        tokio::select! {
            next = stream.next() => match next {
                Some(next) => {
                    apply(&mut view, &mut follower, &frame(&next?)?, &mut output).await?;
                }
                None => return Ok(()),
            },
            line = lines.next_line() => match line? {
                Some(line) => {
                    let waiting = view
                        .as_ref()
                        .and_then(View::waiting_permission)
                        .filter(|permission| answered.as_ref() != Some(&permission.id));
                    let text = match waiting.and_then(|permission| {
                        selected_option(&line, &permission.options)
                            .map(|option| (permission.id.clone(), option))
                    }) {
                        Some((permission, option)) => {
                            answered = Some(permission);
                            option
                        }
                        None => line,
                    };
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
            follower.admit(&[], &fresh);
            let text = follower.snapshot(&[], &fresh, &HashMap::new());
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

/// A permission question accepts its displayed number. The selected option
/// identifier becomes the next prompt, just as text entered at the console.
fn selected_option(line: &str, options: &[PermissionOptionDto]) -> Option<String> {
    let index = line.trim().parse::<usize>().ok()?.checked_sub(1)?;
    options.get(index).map(|option| option.id.clone())
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
        /// The steps of each turn once every stream script is taken: what
        /// the store holds after a reconnect.
        steps_after: HashMap<String, Value>,
        streams: Arc<Mutex<VecDeque<Script>>>,
        prompts: Arc<Mutex<Vec<String>>>,
        /// The `before` of each page read, `-` for the newest.
        reads: Arc<Mutex<Vec<String>>>,
        /// Every stream stays open after its frames, as a quiet session.
        hold: bool,
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
        let after = stub.streams.lock().unwrap().is_empty();
        let steps = match after {
            true => stub.steps_after.get(&turn_id).or(stub.steps.get(&turn_id)),
            false => stub.steps.get(&turn_id),
        };
        Json(json!({"steps": steps.cloned().unwrap_or(json!([])), "earlier_steps": 0}))
    }

    async fn trajectory_stream(
        State(stub): State<Stub>,
    ) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
        let script = stub.streams.lock().unwrap().pop_front();
        // A connection past the last script holds still, as a quiet session.
        let open = stub.hold || script.is_none();
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
            "steps": [], "earlier_steps": 0, "attention": [], "attention_more": 0, "notices": [], "notices_more": 0,
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

    fn error(id: &str, turn_id: &str) -> Value {
        json!({"type": "error", "id": id, "turn_id": turn_id, "lane": "main",
               "created_at": "2026-11-27T10:03:00Z", "text": format!("error {id}"),
               "diagnosis": null})
    }

    async fn follow_with(stub: Stub, layout: Layout, steps: bool, filters: Filters) -> String {
        let (client, server) = serve(stub).await;
        let mut follower = Follower::new(layout, steps, filters, Printer::new(Utc));
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
        server.abort();
        out
    }

    fn streams(scripts: Vec<Script>) -> Arc<Mutex<VecDeque<Script>>> {
        Arc::new(Mutex::new(VecDeque::from(scripts)))
    }

    #[tokio::test]
    async fn kind_narrows_the_attention_a_folded_turn_shows() {
        let stub = Stub {
            pages: HashMap::from([(
                "-".into(),
                snapshot(
                    0,
                    "idle",
                    vec![turn(
                        "t1",
                        "failed",
                        json!({"attention": [error("e1", "t1")]}),
                    )],
                    false,
                ),
            )]),
            ..Stub::default()
        };
        let (client, server) = serve(stub).await;
        let printer = Printer::new(Utc);
        let only = |kind: &str| Filters {
            kinds: vec![kind.into()],
            ..Filters::default()
        };

        let calls = transcript::read(&client, "s", &only("tool_call"), false)
            .await
            .unwrap();
        let errors = transcript::read(&client, "s", &only("error"), false)
            .await
            .unwrap();

        assert!(!table(&printer, &calls).contains("error e1"));
        assert!(table(&printer, &errors).contains("  10:03:00  error  error e1\n"));
        server.abort();
    }

    #[tokio::test]
    async fn a_follow_reads_the_older_turns_its_tail_reaches() {
        let stub = Stub {
            streams: streams(vec![vec![(
                "snapshot",
                snapshot(0, "exited", vec![turn("t3", "completed", json!({}))], true),
            )]]),
            pages: HashMap::from([(
                "t3".into(),
                snapshot(
                    0,
                    "exited",
                    vec![
                        turn("t1", "completed", json!({})),
                        turn("t2", "completed", json!({})),
                    ],
                    false,
                ),
            )]),
            ..Stub::default()
        };
        let filters = Filters {
            tail: Some(2),
            ..Filters::default()
        };

        let out = follow_with(stub, Layout::Lines, false, filters).await;

        assert_eq!(
            out,
            "prompt · prompt t2\n\
             turn · completed · 2 steps · 2 tool calls\n\
             answer · answer t2\n\
             prompt · prompt t3\n\
             turn · completed · 2 steps · 2 tool calls\n\
             answer · answer t3\n"
        );
    }

    #[tokio::test]
    async fn a_reconnect_prints_the_turns_that_came_and_went_while_the_stream_was_down() {
        let stub = Stub {
            streams: streams(vec![
                vec![(
                    "snapshot",
                    snapshot(0, "running", vec![turn("t1", "running", json!({}))], false),
                )],
                vec![(
                    "snapshot",
                    snapshot(7, "exited", vec![turn("t3", "completed", json!({}))], true),
                )],
            ]),
            pages: HashMap::from([(
                "t3".into(),
                snapshot(
                    0,
                    "exited",
                    vec![
                        turn("t1", "completed", json!({})),
                        turn("t2", "completed", json!({})),
                    ],
                    true,
                ),
            )]),
            ..Stub::default()
        };
        let reads = stub.reads.clone();

        let out = follow_with(stub, Layout::Lines, false, Filters::default()).await;

        assert_eq!(
            out,
            "prompt · prompt t1\n\
             turn · completed · 2 steps · 2 tool calls\n\
             prompt · prompt t2\n\
             turn · completed · 2 steps · 2 tool calls\n\
             answer · answer t2\n\
             prompt · prompt t3\n\
             turn · completed · 2 steps · 2 tool calls\n\
             answer · answer t3\n"
        );
        assert_eq!(
            *reads.lock().unwrap(),
            ["t3"],
            "the page that reaches t1, a turn it knows, is the last one read"
        );
    }

    #[tokio::test]
    async fn a_json_follow_prints_one_typed_turn_in_each_line_with_its_steps() {
        let stub = Stub {
            streams: streams(vec![vec![
                (
                    "snapshot",
                    snapshot(
                        0,
                        "running",
                        vec![turn(
                            "t1",
                            "running",
                            json!({"steps": [call("c1", "t1", "completed")]}),
                        )],
                        false,
                    ),
                ),
                (
                    "update",
                    json!({"rev": 1, "ops": [
                        {"op": "item_upsert", "step": call("c2", "t1", "running")},
                        {"op": "item_upsert", "step": call("c2", "t1", "completed")},
                        {"op": "turn_upsert", "turn": turn("t1", "completed", json!({}))},
                        {"op": "session_patch", "session": session("exited")},
                    ]}),
                ),
            ]]),
            steps: HashMap::from([("t1".into(), json!([call("c1", "t1", "completed")]))]),
            ..Stub::default()
        };

        let out = follow_with(stub, Layout::Json, true, Filters::default()).await;

        let turns: Vec<TurnDto> = out
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(turns.len(), 1, "{out}");
        let ids: Vec<_> = turns[0]
            .steps
            .iter()
            .map(ariadne_console::trajectory::step_id)
            .collect();
        assert_eq!(ids, ["c1", "c2"]);
        assert_eq!(turns[0].earlier_steps, 0);
    }

    #[tokio::test]
    async fn a_number_after_another_client_answered_the_permission_is_a_prompt() {
        let answered = {
            let mut step = permission("p1", "t1");
            step["state"] = json!("answered");
            step["answer"] = json!({"option_id": "yes", "label": "Allow", "decided_by": "console"});
            step
        };
        let stub = Stub {
            streams: streams(vec![vec![
                (
                    "snapshot",
                    snapshot(
                        0,
                        "running",
                        vec![turn(
                            "t1",
                            "running",
                            json!({"steps": [permission("p1", "t1")],
                                   "attention": [permission("p1", "t1")]}),
                        )],
                        false,
                    ),
                ),
                (
                    "update",
                    json!({"rev": 1, "ops": [{"op": "item_upsert", "step": answered}]}),
                ),
            ]]),
            hold: true,
            ..Stub::default()
        };
        let prompts = stub.prompts.clone();
        let (client, server) = serve(stub).await;
        let (mut input_writer, input_reader) = duplex(128);
        let (output_writer, mut output_reader) = duplex(4096);
        let console = tokio::spawn(async move {
            run_with_io(&client, "s", BufReader::new(input_reader), output_writer).await
        });

        let mut output = String::new();
        while !output.contains("permission · answer: Allow (console)\n") {
            let mut buffer = [0; 1024];
            let read = output_reader.read(&mut buffer).await.unwrap();
            assert!(read > 0, "the console ended early: {output}");
            output.push_str(std::str::from_utf8(&buffer[..read]).unwrap());
        }
        input_writer.write_all(b"1\n").await.unwrap();
        input_writer.shutdown().await.unwrap();
        console.await.unwrap().unwrap();

        assert_eq!(*prompts.lock().unwrap(), ["1"]);
        server.abort();
    }

    fn question(id: &str, turn_id: &str, state: &str) -> Value {
        json!({"type": "question", "id": id, "turn_id": turn_id, "lane": "main",
               "created_at": "2026-11-27T10:02:00Z", "request_id": id, "hint": "",
               "question": "Which base?", "options": [], "recommended": null,
               "recommendation_reason": null, "state": state,
               "answer": if state == "answered" { json!("base: next") } else { Value::Null }})
    }

    #[tokio::test]
    async fn a_question_that_closes_while_the_stream_is_down_prints_its_close_after_the_reconnect()
    {
        for steps in [false, true] {
            let stub = Stub {
                streams: streams(vec![
                    vec![(
                        "snapshot",
                        snapshot(
                            0,
                            "running",
                            vec![
                                turn(
                                    "t1",
                                    "completed",
                                    json!({"attention": [question("q1", "t1", "open")]}),
                                ),
                                turn("t2", "running", json!({})),
                            ],
                            false,
                        ),
                    )],
                    vec![(
                        "snapshot",
                        snapshot(
                            5,
                            "exited",
                            vec![
                                turn("t1", "completed", json!({})),
                                turn("t2", "completed", json!({})),
                            ],
                            false,
                        ),
                    )],
                ]),
                steps: HashMap::from([("t1".into(), json!([question("q1", "t1", "open")]))]),
                steps_after: HashMap::from([(
                    "t1".into(),
                    json!([question("q1", "t1", "answered")]),
                )]),
                ..Stub::default()
            };

            let out = follow_with(stub, Layout::Blocks, steps, Filters::default()).await;

            assert_eq!(
                out,
                "10:00:00  USER  prompt t1\n\
                 \x20 10:02:00  question  Which base?\n\
                 \x20 completed · 2 steps · 2 tool calls\n\
                 10:05:00  AGENT  answer t1\n\
                 \n\
                 10:00:00  USER  prompt t2\n\
                 \x20 in the turn of 10:00:00: prompt t1\n\
                 \x20 10:02:00  question  Which base?\n\
                 \x20   answer: base: next\n\
                 \x20 completed · 2 steps · 2 tool calls\n\
                 10:05:00  AGENT  answer t2\n\
                 \n",
                "with --steps: {steps}"
            );
        }
    }

    #[tokio::test]
    async fn a_question_that_closes_on_the_live_stream_after_its_turn_closed_prints_its_close() {
        for steps in [false, true] {
            let stub = Stub {
                streams: streams(vec![vec![
                    (
                        "snapshot",
                        snapshot(
                            0,
                            "running",
                            vec![
                                turn(
                                    "t1",
                                    "completed",
                                    json!({"attention": [question("q1", "t1", "open")]}),
                                ),
                                turn("t2", "running", json!({})),
                            ],
                            false,
                        ),
                    ),
                    (
                        "update",
                        json!({"rev": 1, "ops": [
                            {"op": "item_upsert", "step": question("q1", "t1", "answered")},
                            {"op": "turn_upsert", "turn": turn("t1", "completed", json!({}))},
                        ]}),
                    ),
                    (
                        "update",
                        json!({"rev": 2, "ops": [
                            {"op": "turn_upsert", "turn": turn("t2", "completed", json!({}))},
                            {"op": "session_patch", "session": session("exited")},
                        ]}),
                    ),
                ]]),
                steps: HashMap::from([("t1".into(), json!([question("q1", "t1", "open")]))]),
                ..Stub::default()
            };

            let out = follow_with(stub, Layout::Blocks, steps, Filters::default()).await;

            assert_eq!(
                out,
                "10:00:00  USER  prompt t1\n\
                 \x20 10:02:00  question  Which base?\n\
                 \x20 completed · 2 steps · 2 tool calls\n\
                 10:05:00  AGENT  answer t1\n\
                 \n\
                 10:00:00  USER  prompt t2\n\
                 \x20 in the turn of 10:00:00: prompt t1\n\
                 \x20 10:02:00  question  Which base?\n\
                 \x20   answer: base: next\n\
                 \x20 completed · 2 steps · 2 tool calls\n\
                 10:05:00  AGENT  answer t2\n\
                 \n",
                "with --steps: {steps}"
            );
        }
    }

    /// A session that starts: the daemon sends the note of `session_start`,
    /// then the header of the turn it opens, in one update (040, `finish`).
    fn starting() -> Vec<Script> {
        let pre_prompt = |state: &str| {
            turn(
                "t0",
                state,
                json!({"prompt": null, "answer": [],
                       "activity": {"steps": 0, "tool_calls": 0, "files_changed": 0, "failed": 0,
                                    "cancelled": 0, "thoughts": 0, "subagents": 0}}),
            )
        };
        let started = json!({"type": "note", "id": "n0", "turn_id": "t0", "lane": "main",
                             "created_at": "2026-11-27T10:00:00Z", "origin": "session_start",
                             "severity": "info", "text": "session started"});
        vec![vec![
            ("snapshot", snapshot(0, "starting", vec![], false)),
            (
                "update",
                json!({"rev": 1, "ops": [
                    {"op": "item_upsert", "step": started},
                    {"op": "turn_upsert", "turn": pre_prompt("running")},
                ]}),
            ),
            (
                "update",
                json!({"rev": 2, "ops": [
                    {"op": "turn_upsert", "turn": pre_prompt("interrupted")},
                    {"op": "session_patch", "session": session("exited")},
                ]}),
            ),
        ]]
    }

    #[tokio::test]
    async fn the_first_step_of_a_starting_session_prints_under_its_turn() {
        let stub = Stub {
            streams: streams(starting()),
            ..Stub::default()
        };

        let out = follow_with(stub, Layout::Blocks, true, Filters::default()).await;

        assert_eq!(
            out,
            "10:00:00  SESSION  before the first prompt\n\
             \x20 10:00:00  note  session started\n\
             \x20 interrupted\n\
             \n"
        );
    }

    #[tokio::test]
    async fn a_json_follow_keeps_the_first_step_of_a_starting_session_in_its_turn() {
        let stub = Stub {
            streams: streams(starting()),
            ..Stub::default()
        };

        let out = follow_with(stub, Layout::Json, true, Filters::default()).await;

        let turns: Vec<TurnDto> = out
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(turns.len(), 1, "{out}");
        let ids: Vec<_> = turns[0]
            .steps
            .iter()
            .map(ariadne_console::trajectory::step_id)
            .collect();
        assert_eq!(ids, ["n0"]);
    }

    #[tokio::test]
    async fn a_piped_console_prints_the_first_step_of_a_starting_session() {
        let stub = Stub {
            streams: streams(starting()),
            hold: true,
            ..Stub::default()
        };
        let (client, server) = serve(stub).await;
        let (mut input_writer, input_reader) = duplex(128);
        let (output_writer, mut output_reader) = duplex(4096);
        let console = tokio::spawn(async move {
            run_with_io(&client, "s", BufReader::new(input_reader), output_writer).await
        });

        let mut output = String::new();
        while !output.contains("turn · interrupted\n") {
            let mut buffer = [0; 1024];
            let read = output_reader.read(&mut buffer).await.unwrap();
            assert!(read > 0, "the console ended early: {output}");
            output.push_str(std::str::from_utf8(&buffer[..read]).unwrap());
        }
        input_writer.shutdown().await.unwrap();
        console.await.unwrap().unwrap();

        assert_eq!(
            output,
            "session · before the first prompt\nnote · session started\nturn · interrupted\n"
        );
        server.abort();
    }
}
