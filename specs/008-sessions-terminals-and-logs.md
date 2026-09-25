---
id: sessions-terminals-and-logs
status: current
updated: 2026-09-24
areas: [daemon, store, cli]
commits: [e4816cf6, 39937143, a69b953f]
tests:
  - crates/ariadne-daemon/src/bus.rs
  - crates/ariadne-daemon/src/http/console.rs
  - crates/ariadne-daemon/tests/it/resume.rs
  - crates/ariadne-daemon/tests/it/acp_console.rs
  - crates/ariadne-daemon/tests/it/acp_terminal.rs
  - crates/ariadne-daemon/tests/it/acp_runtime.rs
  - crates/ariadne-daemon/tests/it/events.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/console.rs
  - crates/ariadne-cli/src/commands/console/tui.rs
  - crates/ariadne-cli/src/commands/transcript.rs
  - crates/ariadne-console/src/ansi.rs
  - crates/ariadne-console/src/markdown.rs
  - crates/ariadne-console/src/transcript.rs
  - crates/ariadne-console/src/tui/mod.rs
  - crates/ariadne-console/src/tui/blocks.rs
  - crates/ariadne-console/src/tui/chrome.rs
  - crates/ariadne-console/src/tui/input.rs
  - crates/ariadne-console/src/tui/picker.rs
  - crates/ariadne-console/src/tui/viewport.rs
  - crates/ariadne-console/src/tui/scenario.rs
  - crates/ariadne-console/src/theme.rs
---

# Sessions and the console

An agent session is a row the daemon keeps for one agent, optionally on a seat, and the
agent behind it is a daemon-owned ACP child process (021). This is what a
session holds, what may be done to it, and how a client reads it and speaks
to it: through the session's console.

## Scope

In: the session row and its statuses, launches and relaunches of one row,
kill and resume, the console — its transcript, its live stream and posted
input — how the CLI reaches the console, and the console served as a
terminal over a WebSocket for the desktop app.

Out: the ACP runtime itself — the child process, the protocol conversation,
permission modes and the event vocabulary (021); sessions started outside
Ariadne and resumed as loose sessions (020); when the daemon hands an agent a prompt
(009) and what the prompt says (006); how `ariadne attach` resolves a task or
goal id to a seat (014).

## Behavior

1. A staffed session belongs to a goal and seat, and authors and reviewers also belong to a task.
   A loose session has no goal, task or seat (020).
2. A session holds its worktree, the model it runs on, the effort where one
   was pinned, and the agent's own session id once the agent reports one.
3. The model is one `<agent>:<model>` pin (011), frozen off the seat's pin
   when the session is created. The agent is the registry id before the
   pin's first `:`, and no agent kind is stored beside it.
4. Every session's agent is an ACP child process that the daemon owns. No
   session has a terminal, a pane or a grid.
5. A session is `starting`, `running`, `idle`, `exited` or `failed`. The
   first three are live.
6. Sessions are long-lived: one author per task, one reviewer per task across
   every review of it, one orchestrator per goal. Restarting one reopens the
   same row.
7. Every launch of a session is dated, and every launch runs under a launch
   id of its own. The id is written to the row before the agent starts and is
   handed to the agent's MCP server as `ARIADNE_LAUNCH_ID`.
8. A report from a launch the row has moved past changes nothing (012). A
   relaunch has two processes under one session id for as long as the old
   one takes to exit, and only the launch id tells their reports apart. The
   old process's `session_end` is not recorded: the session has not ended,
   and every console closes on a `session_end`.
9. A relaunch announces the session as updated on the event stream.
10. Killing a session kills its agent process and marks a live session
    `exited`. The conversation stays with the agent, so the session can be
    resumed. A status an event decided before the kill cannot land after it:
    the write is guarded against the row it touches, not the one its caller
    read (012).
11. Resuming a session revives it in place: the same row, the same id, the
    same model, on the conversation its agent id names.
    A missing worktree falls back to the repository checkout.
    The daemon creates no replacement worktree for this revival.
12. A session with no agent id to resume from is not revived.
    A session of a completed goal can revive, and the goal stays completed.
    The scheduler leaves that revived conversation running until the daemon stops.
    A session of a cancelled goal is not revived.
13. A session's console snapshot (`GET /v1/sessions/{id}/console`) is the
    newest page of the session's events, in order: at most two hundred of
    them, the recent past rather than every turn the session ever ran. A
    session that has worked for hours holds thousands of events and a tool
    end averages ten kilobytes, so the whole of one is tens of megabytes to
    read and to send; a client that wants what is below the page walks back
    from it with `GET /v1/events` (012). Every event the runtime reported
    passes through as the runtime named it, with no fixed list of kinds. A
    tool call is two stored events and the page keeps the two together: an
    end whose start the page cut off is dropped, because the input the call
    was made with is on its start alone (021).
    While a turn runs, the snapshot holds the text so far: the run of text
    the agent is still writing (021), as one `agent_thought_chunk` or
    `agent_message_chunk`, and only where there is one. The runs before it
    are stored events, in their place among the calls. The text so far is
    read first, under the runtime's turn lock, and the stored events after it, and the snapshot is the two in id
    order: the chunks take fresh ids as they are read, so they fall after
    every event stored before the read and before every event stored after
    it — a turn that ended, or a prompt that began, between the two reads is
    in the snapshot, in its place, rather than missing from it and arriving
    later behind the text. A live event sent between the two reads — the last
    chunk of a turn that ended in between, the progress of a call that ended —
    has an id below the last stored event read, and joins the snapshot in its
    place too; one sent after the stored events were read follows the
    snapshot on the stream. A live channel that dropped events while the
    stored ones were read has lost text the snapshot cannot show: the
    snapshot goes out as it is, and the first thing the stream says after it
    is a resync (rule 15). `GET /console` has no stream to say resync on: it
    opens again while the channel lags under the open, and when the channel
    lags every time it refuses with 503 rather than answer short. Between
    turns the snapshot is the stored events alone.
14. The console stream (`GET /v1/sessions/{id}/console/stream`) opens with
    that snapshot, then sends each later event as it is recorded, and each
    live-only event as the runtime streams it (021): message and thought
    chunks, and tool call progress. The stored and the live events go out in
    the order the daemon gave them ids, which one monotonic generator gives
    both: whenever both channels hold events, they are sorted by id before
    they go out, so a stored whole never passes the live chunk that preceded
    it and a live chunk never passes the stored prompt that began its turn.
    A client that connects mid-turn reads the text so far in its snapshot and
    every later chunk on the stream, none of them twice. A stored event
    reaches the stream a moment after it is committed, because the bus loads
    what the event carries before it publishes it, so a live event can be in
    hand while a stored event below it is not. The stream does not read the
    store for it: it waits for the bus to publish everything it held when
    the live event arrived, which the bus answers for, and sends what that
    gives it in id order. So a console costs the store one read, for the
    page it opened on, and following a turn of streamed text costs none.
    An event whose payload the bus cannot load is dropped there and reaches
    no stream (012), and no later event brings it: the bus counts what it
    dropped under the session the event belonged to, and a console of that
    session says resync (rule 15) rather than send a live event over the
    hole. It reads the count once the bus has answered for what it held and
    before it sends anything, because the answer is what publishes the
    events below the live one, and so where one of them is dropped. A
    console of any other session reads its own count, and goes on.
15. The console stream has no replay. A client that falls too far behind is
    told how many events it missed, and the connection closes, the same as
    `/v1/events/stream` (012). A client the bus dropped an event for is told
    the same way, with the count of what was dropped. A reconnect starts
    again from a fresh snapshot, read from the store, which holds them.
16. Console input (`POST /v1/sessions/{id}/console/input`) becomes a
    `session/prompt`. An agent runs one turn at a time, so input posted while
    a turn runs is queued and sent the moment the turn ends, in the order it
    was posted.
17. Console input that answers a pending permission question is taken as the
    selected option (021).
18. Console input takes down whatever the session was flagged for: an answer
    is an answer, whoever gave it.
19. A finished session refuses console input with `409`, and so does a
    session whose agent process is gone.
20. `POST /v1/sessions/{id}/console/cancel` cancels the turn the session is
    running (021) and answers `204`. A session that is not live, one whose
    agent process is gone, and one between turns have nothing to cancel, and
    answer `409`.
21. A session's console is also served as a terminal
    (`GET /v1/sessions/{id}/console/terminal`), for a client that is a
    terminal emulator rather than a terminal: the desktop app's xterm.js.
    The request upgrades to a WebSocket; an id that names no session
    answers `404` before the upgrade. The daemon runs the console of rules
    22 to 29 itself, in process, on a ratatui backend of its own that writes
    the escape sequences a terminal reads and answers the cursor position
    and the size from what it wrote and what the client said, so no query
    ever goes to a terminal; the scrollback is the emulator's. The protocol
    is defined once, in `ariadne-api` (`TerminalClientMessage`,
    `TerminalServerMessage`). Client to server, as JSON text frames: a
    `resize` with `cols` and `rows`, sent first — nothing is drawn before it
    — and on every change, a size of no rows or no columns counting as one;
    a `key` with a `code` (a printable character as `{"char": "a"}`, a
    function key as `{"f": 5}`, and `enter`, `backspace`, `tab`, `back_tab`,
    `esc`, `left`, `right`, `up`, `down`, `home`, `end`, `page_up`,
    `page_down`, `delete`, `insert` by name) and its `modifiers` (`shift`,
    `control`, `alt`, `super`, `hyper`, `meta`), one to one onto crossterm's
    `KeyEvent`; and a `paste` with its `text`. Server to client: binary
    frames of the terminal bytes that draw the console, and a text frame
    `{"type": "status", "status": ...}` with the session's status as the
    socket opens and as the console ends. The console's events are the
    snapshot and the stored and live events of rules 13 and 14, from the
    same channels; a receiver that fell behind says the stream dropped and
    subscribes again from a fresh snapshot, as the CLI does. What is typed
    and Escape take the paths of rules 16 to 20, so a permission answer, a
    queued prompt and the attention flag behave the same whichever console
    it was. The console runs while the socket is open: the client closing
    it ends the console and frees the session's receivers, and the session
    stays alive; the session ending sends the last bytes, the final status
    and closes the socket, and a socket opened after the session ended
    draws the transcript and closes on it the same way. A session has ended
    where its last `session_end` has no `session_start` after it: a relaunch
    is not an end, so a socket open over one stays open, and so does a
    socket opened on a session revived after it ended; Ctrl-C twice and
    Ctrl-D over the socket close it too, and the session stays alive.
22. The CLI reaches a session through its console. `ariadne attach` renders
    the transcript, follows the stream and posts what is typed as input.
    `ariadne session logs` and `ariadne task logs` print the snapshot as typed
    transcript blocks. Paired tool and permission events form one block;
    agent text stays whole, and diffs retain their line colouring. `--tail`,
    `--since` and `--kind` filter the snapshot, which is the page of rule 13
    and no more. With `--follow`, chunks stream under one item header and
    `--kind` also filters later events. JSON output keeps each event object
    unchanged.
23. `ariadne attach` on a terminal is an inline pane, never the alternate
    screen. A finished block goes into the terminal's own buffer above the
    pane, so it stays in the scrollback; the pane holds the block still being
    written, a status row, the input box and a footer. While a turn runs the last
    block is the one being written; between turns none is, and the last one
    is committed too — but a run of chunks no stored whole has closed, which
    the next chunk would continue. The pane is as tall as what it
    holds — the lines of the blocks not yet in the scrollback, and the pinned
    rows under them: the status row, the input box and the footer — and the terminal's
    height at the most, where it shows the last lines of the block. It grows
    as the block being written grows and shrinks as blocks leave for the
    scrollback, so between turns the pane is its pinned rows alone, and no
    blank row lies between the last line of the scrollback and the pane but
    the one that separates blocks. ratatui
    fixes an inline viewport's height as it opens it, so a pane whose height
    changes is opened again over the same backend, from the row it began on:
    a taller one scrolls the terminal for the room it lacks, and a shorter
    one frees the rows its finished blocks were written on, which are then
    inserted onto them. No committed line is deleted or said again by
    either, and the pane is wiped and drawn whole only then — every other
    draw writes the cells that changed. ratatui drops the backend where
    opening the pane again fails: the console ends on that error, and the
    close that follows writes nothing and does not panic. A pending permission question has
    the terminal's height less the pinned rows to fold into (rule 27). The
    status row names the seat,
    the model and the session's status — the row's at attach, then what the
    stored events move it to, as the daemon moves the row on them (021):
    `running` on the session's start, a prompt, a tool event or an answered
    permission, `idle` on a stop, `exited` on the session's
    end. A row that had ended when the console attached — `exited` or
    `failed` — stays so whatever the events replayed under it say, since the
    daemon moves a live row only and a row its sweep ended has no
    `session_end` stored; a live row's replay may hold an earlier launch's
    `session_end` before this launch's `session_start`, and follows both, so
    a resumed session reads as the daemon has it; while the stream is down
    the status reads `reconnecting`. The status row also turns
    a spinner with "thinking" or
    "running &lt;tool&gt;" while a turn runs, followed by how long the turn
    has run — `12s`, or `1m 04s` past a minute — counted from the event that
    began it, so a turn already running at attach counts from its prompt;
    the count starts again with each turn, survives a reconnect's replay,
    and is absent between turns. A prompt typed while a turn runs leaves the
    row as it was. The footer holds, on the left, the key
    hints of the state the console is in — a Ctrl-C armed to leave: `ctrl-c
    again to leave`; a pending question: `up/down or 1-9 choose · enter answer`;
    a running turn: `enter send · shift+enter newline · esc cancel · ctrl-c
    quit`; idle: `enter send · shift+enter newline · ctrl-c quit` — and on the
    right the tokens the session has spent, `↑ <input> ↓ <output>` in
    compact numbers (`950`, `12.4k`, `1.2M`), drawn nowhere both are zero.
    The tokens are the session's, as `SessionDto.usage` is: the sum over its
    launches of each launch's last totals. At attach they are the row's
    `usage`; each event that carries an `ariadne_usage` — a `stop` does —
    holds its launch's totals so far, keyed by `source`, and replaces that
    launch's figure. The footer draws the sum of those figures, or the row's
    at attach where that is more, counter by counter: the row can hold a
    figure read mid-turn, ahead of the launch's last stop. Neither row is
    ever cut. Each is made of parts measured by display width, and a row
    wider than the pane drops whole parts, the least important first: on the
    status row the model, then the seat, the turn's clock, the spinner with
    what the turn is doing, and the session's status last; on the footer the
    hints after the first, the last of them first, then the tokens, and the
    first hint — the armed notice is the only one of its state — last.
    Every block, the agent's markdown and the
    input box wrap and cut by display width, so a wide character or an emoji
    takes the two columns it draws on, and a cut falls between grapheme
    clusters, so an emoji of several characters is never split. A resize of
    the terminal redraws the pane at the new size, by the same height rule:
    a pane that ran off the bottom of a shorter terminal is opened again as
    many rows further up, so the whole of it is on the screen.
    A narrower terminal has the pane opened again on its top row: the
    pane's rows are erased, and the rows above it are scrolled into the
    scrollback, never erased. An erase from the top-left corner to the end
    of the screen is made one row at a time, since tmux keeps a copy of what
    that erase removes in its scrollback. The blank end of each row is
    erased, not written as spaces, so a terminal made narrower does not wrap
    blank cells into rows of their own. The console the daemon hosts for
    the desktop app does not fit in place on a resize: its emulator holds
    the console and nothing else, so once the size has held for 250 ms it
    clears the screen and the scrollback and draws the banner, every block
    and the pane again at the new size. The CLI's terminal holds the
    user's shell above the console, and fits in place. The pane
    opens from the
    cursor, which the terminal is asked for once, at the open; a terminal that
    does not answer within crossterm's timeout gets the same pane opened from
    the bottom row instead. On both paths every later cursor query — the one
    ratatui makes after each block it inserts above the pane, on a resize,
    and each time the pane is opened again at another height — is answered
    by the backend itself, from where it last put the cursor,
    and never sent to the terminal: once the key stream reads the terminal, a
    query's answer would come through the reader the stream holds, and time
    out. There is no alternate-screen fallback.
    Each attach puts one welcome banner into scrollback before its first
    transcript block, with one blank line under it: seat, task title (or
    `orchestrator` with the goal title),
    model and effort, repository name, and the session id cut to 12 columns.
    The frame fits its contents but no more than the pane; a long title cuts
    at a grapheme boundary with `…`, and a pane narrower than 40 columns uses
    unframed lines. A reconnect snapshot does not print the banner again, and
    an unavailable task or repository leaves out only that line. The CLI reads
    this context over its HTTP client; the terminal socket host reads it from
    daemon state. Redirected CLI attach remains the plain line protocol.
24. The pane renders each block as it arrives, one blank line between two
    blocks in the live area as in the scrollback. A typed prompt draws as
    `❯ text`, the input box's glyph, with a bar `▌` in the user's colour down
    the left edge of each of its rows and its text bold in the terminal's own
    foreground, so it stands out on a dark and a light theme. It holds the
    event's `text` alone — never the whole `prompt` with the system prompt
    ahead of it (021), nor the summary, one line cut short; an event carrying
    no `text` says the text was not recorded rather than draw the whole. A
    prompt typed and not yet taken back as its `user_prompt_submit` carries a
    dim `queued` tag, which goes when the event comes back. A prompt the
    daemon sent (`source: daemon`: a briefing, a nudge, a message) draws
    under its own marker and label, `» daemon`, with its first 6 lines
    beneath and a count of the rest (`… 34 more lines`) while the console's
    fold state is folded, whole once Ctrl-O sets it whole for the rest of the
    attach (rule 27), so it reads apart from what was typed. Agent text is
    markdown chunk by chunk under one marker, a thought dimmed and folded to
    4 lines under the same fold state, a tool call the block of the next
    rule, and a permission question a picker. Markdown tables align their
    display-width cells under bold headers — not underlined, with one dim
    rule as wide as the row beneath it — and wrap at spaces in a cell: a
    table wider than the pane keeps each column that fits its share at its
    own width and splits what is left among the others, a code span
    keeping its backticks there as in text, or become
    `header: value` lines where the pane is too narrow. Fenced code has a dim
    language label, a two-column code indent and a dim `↪` on continued lines,
    never a fence. Links retain their plain URL, lists use `•`, `◦` and `▪` by
    depth with task markers, and every wrapped quote line keeps its `│ ` bar.
    Each mark of the transcript has one meaning over the whole pane. The
    renderer is pure, uses no syntax colour or OSC 8 link, and accepts
    each incomplete markdown prefix without a panic. A plan is a checklist
    under a head that counts the completed entries, `plan 1/3`: `☐` pending,
    `◐` in progress in the plan colour, `☑` completed and dimmed, a long entry
    wrapped under its own text. An error keeps `✗` and wraps whole: a row
    keeps the space after its last word, a word wider than the pane is cut
    between grapheme clusters across rows, and no character is lost. A note
    is in plain words — `session started`,
    `session ended`, `context compacted`, `turn cancelled`, and any other
    stop reason as words (`turn stopped: token limit reached`); a stop with
    reason `end_turn` draws nothing, in the pane and in `ariadne session
    logs` alike. An event of a kind the pane has no block for draws dimmed
    as `<kind> · <summary>`, from the event's summary.
25. A tool call reads as a coding agent's. Its head line is a status glyph —
    `○` pending, `◐` in progress (the plan's mark for it), `✓` completed,
    `✗` failed — then a glyph
    for the ACP `kind` (`$` execute, `≡` read, `✎` edit, `⌫` delete, `→`
    move, `⌕` search, `⇣` fetch, `∴` think, `⇄` switch mode, `◇` otherwise)
    and what the call is about, taken from its input: the command for
    `execute`, the path and line for `read`, `edit`, `delete` and `move`, the
    pattern and where it is looked for for `search`, the URL for `fetch`, and
    the call's title where the kind names nothing. Raw JSON is never drawn
    where a field names the subject, and `locations` the head does not already
    name follow it. Once the call has ended, the head carries the time from
    the event that opened it to the one that ended it. A live
    `tool_call_update` merges into the open call with the same `toolCallId`:
    one block per call, however many updates arrive. While the turn runs, a
    call that has not ended stays in the pane, and so does everything after
    it: the question
    about a call comes after the call and the event that ends it after the
    answer, so the scrollback gets the call as it ended, never as pending.
    A call the turn left open when it stopped — a cancelled turn leaves
    one — holds nothing back.
    The output is the
    call's `rawOutput` where that is text or a stdout and stderr pair, the
    text of its `content` entries otherwise, and the structure as JSON only
    where there is neither. It is folded to its last lines with a count of
    the hidden ones, trailing blank lines trimmed, under the console's fold
    state (rule 27); the fold is the thought's. Its first row starts with
    `⎿ ` two columns under the head's mark, which
    ties it to the call, and its next rows and the diff's rows start at the
    column of its text.
    A tab in the output takes the columns to the next stop of eight, since a
    cell drawn with a tab draws nothing. A `diff` content entry draws as a
    unified diff under a file header — the path, or the old name to the new
    where they differ — with added, removed, hunk and context lines each in
    their own colour, folded past a line limit with a count, under the same
    fold state. Where the agent
    sent `oldText` and `newText` rather than a patch, the diff is the hunks
    between them with three lines of context, never every old line and then
    every new one. A patch that starts at its first hunk takes its file
    header from the entry's `path`. A permission question draws the call it asks about under
    the question — the same head line, then the rest of a command that has
    more than one line, or the diff — above its options. A question still
    open is framed by a rule with the label `permission` in the ask colour
    above it and a rule below its options. The question is in bold. The
    picked option starts with `❯ `, and its number and name are in bold in
    the ask colour; the others start with two spaces and have their numbers
    dimmed. A question or an option name wider than the pane wraps and is
    cut nowhere, between grapheme clusters where a word is wider than the
    row. Once answered, the block is the question, the head line of the call
    and `↳ ` and the name of the option chosen: no option list and no frame.
26. A chunk continues the block last written, and starts a block of its own
    where anything else came between: a turn that speaks around a tool call
    reads as two blocks with the call between them. The daemon stores each
    run of text whole once the next thing arrives (021), so a stored run
    closes the block its chunks opened and repeats none of it. A run that
    streamed nothing renders its stored text as a block of its own. A turn
    that ends at once has its last chunks and its stored whole ready together, and
    a stream can hand over the whole first: a chunk that arrives after the
    whole of its kind, with an id below that whole's, and says nothing the
    whole did not is a late chunk of that turn, folded in and not drawn
    again, and it does not reopen the block for the chunk after it. The id is
    what tells it from the next turn's first chunk handed over before its
    prompt: the daemon gives the live chunks and the stored events their ids
    from one monotonic generator, and a new turn's chunk gets its id after
    its prompt, which is after the whole before it.
27. The input box has one dim horizontal rule above its text and one below,
    with no side border or corners. Its first text row starts with `❯ ` and
    every continued row starts with two spaces. An empty box shows the dim
    placeholder `Tell the agent what to do`, which is not input. Text wraps
    between grapheme clusters by display width. The box grows to four visual
    rows and then scrolls down to keep the cursor visible. Up and Down move by
    visual row. From the first or last visual row they move through prompts
    this console sent and console-sourced prompts in its snapshot; moving past
    the newest restores the draft. Daemon-sourced prompts never enter this
    history. Enter posts the input box to console input. Shift+Enter —
    where the terminal reports it, through the keyboard protocol the CLI
    asks for — Alt+Enter and Ctrl-J add a line and post nothing. Enter
    after a backslash
    at the end of the line removes it and adds a line; any other backslash
    stays and Enter posts the prompt. Each prompt typed shows at once and is
    replaced by its own `user_prompt_submit`, in the order they were posted. A prompt typed
    while a turn runs is queued behind it (rule 16), so what that turn says
    after the prompt was typed is drawn above the prompt — the prompt is the
    next turn's — and the block above the prompt stays in the pane while the
    turn writes into it. An older
    daemon's (021) prompt event carries neither `text` nor `source`: it takes
    the oldest pending prompt's place where its whole `prompt` ends in a
    blank line and then the typed text — the whole is the system prompt, a
    blank line and the text, so a daemon prompt that merely ends in the same
    words does not match — keeping what was typed, and is a prompt of its
    own otherwise. The terminal is in bracketed paste, so a paste arrives as
    one event: it goes into the box at the cursor with its line breaks kept,
    and sends nothing until Enter. The box has the shell's line-editing keys:
    Ctrl-A and Ctrl-E to the start and the end of the line, Ctrl-U and Ctrl-K
    deleting to them, Ctrl-W deleting the word before the cursor, and
    Alt-Left and Alt-Right — or Alt-B and Alt-F, which is what a terminal
    that sends the readline sequences for them gives — moving by word.
    Ctrl-O toggles the console's fold state, folded or whole, for the rest of
    this attach — a fresh attach starts folded, and a reconnect keeps
    whichever the attach had. Whole draws every fold of the pane in full: a
    tool's output, a diff, a thought and a daemon prompt (rules 24, 25); a
    block moved to the scrollback while the state is whole carries every
    line of it, since a block already there cannot be drawn again. The
    footer names the state as its last hint, after every other, in its drop
    order (rule 23). On a permission question the arrows and the number keys
    move the pick and Enter posts the option's id; Ctrl-O still toggles the
    fold state there too. While it waits, the picker is the last block
    of the pane, above the box, whatever came after it — a snapshot taken
    mid-turn ends on the text so far (rule 13), which comes after the
    question in it — and its command or diff is folded to the room its
    question, options and two rules leave, whatever the fold state says, so
    the question and every option are on the screen together. A post the
    daemon refuses is said on the transcript, and the console stays open; a
    refused prompt is no longer queued, and holds nothing after it out of the
    scrollback.
28. Escape during a running turn posts to console cancel. Ctrl-C twice, or
    Ctrl-D, leaves the console, and the session stays alive. Every way out
    puts the terminal back: raw mode off, bracketed paste off and the cursor
    shown. The pane is wiped, and the cursor is left on its top row, under
    the last block, where the shell comes back. The CLI then says that the session still
    runs and how to attach again, or, where the session has ended, how to
    revive it.
29. A dropped stream says "reconnecting" and is dialled again on the backoff
    every other follow uses. The fresh snapshot redraws what was open and does
    not repeat what is already in the scrollback.
30. With stdin or stdout redirected there is no pane. `ariadne attach` is then
    the plain line protocol: one `kind · summary` per event, numbered options
    for a permission question, and one prompt per line read (014).

## Acceptance criteria

- A session keeps the model it started on, however the seat's pin moves
  afterwards
  (`resume.rs::a_running_reviewer_keeps_the_model_its_session_started_on`,
  `::a_resumed_author_stays_on_the_model_its_session_started_on`).
- The schema names an agent by its registry id alone, and a session stores no
  agent kind (`store.rs::the_schema_names_agents_by_registry_id_alone`).
- A session's agent runs on the daemon's own stdio
  (`acp_runtime.rs::an_acp_author_runs_on_daemon_stdio`).
- A session row and its events round-trip through the store, and a live one
  is found as live (`store.rs::sessions_and_events_round_trip`).
- Restarting a session reopens the same row
  (`store.rs::restarting_a_session_reopens_the_same_row`), and every launch is
  dated (`::every_launch_of_a_session_is_dated`).
- Every launch reports under an id of its own, carried by the agent's MCP
  server (`resume.rs::every_launch_of_a_session_reports_under_a_new_id`), and
  a report from a launch the row has moved past changes nothing, and its
  `session_end` is not recorded
  (`events.rs::an_event_from_a_launch_the_session_has_moved_past_changes_nothing`).
- A session put back to `starting` for a relaunch has moved past the launch
  it had, in that same write, so the old agent's own reports move nothing
  while the relaunch is on its way
  (`events.rs::a_session_put_back_to_starting_has_moved_past_its_last_launch`).
- A relaunch announces the session as updated
  (`resume.rs::a_relaunch_announces_the_session_as_updated`).
- The author and the reviewer reuse one session across reviews
  (`resume.rs::resuming_the_author_reuses_its_session_across_reviews`,
  `::a_reviewer_reuses_its_session_across_reviews`).
- Killing a session kills its agent process
  (`acp_runtime.rs::killing_an_acp_session_kills_its_agent_process`), and a
  status decided before the kill is never written after it, race included
  (`store.rs::a_status_is_only_ever_written_while_the_session_is_still_live`),
  and neither is one decided under a launch the row has since moved past
  (`store.rs::a_status_is_only_written_for_the_launch_it_was_decided_for`).
- Reviving a session revives it in place
  (`resume.rs::reviving_a_session_revives_it_in_place`); a session without an
  agent id is not revived (`::a_session_without_an_agent_id_is_not_revived`),
  a completed goal's session revives without changing the goal
  (`::a_session_of_a_completed_goal_revives_and_the_goal_stays_completed`),
  and a deleted worktree falls back to the repository checkout
  (`::a_session_with_a_deleted_worktree_revives_in_the_repository_checkout`).
- The console stream gives the snapshot, then deltas
  (`acp_console.rs::the_console_stream_gives_the_snapshot_then_deltas`).
- A console opens on a page: a session of thousands of events gives its
  newest two hundred, in order
  (`http/console.rs::opening_a_console_on_a_long_session_reads_a_page_of_the_newest_events`),
  and the page keeps a tool call whole — the end of a call whose start it cut
  off is dropped, and the call it holds whole still shows its input
  (`::a_page_that_cut_a_calls_start_off_drops_its_end_and_keeps_a_whole_call`),
  each start taken by the first end that matches it, so an end the bound cut
  off cannot live on a later call of the same tool
  (`::an_end_cut_off_from_its_start_does_not_take_the_start_of_a_later_call`).
- A permission request and its reply appear in the console stream
  (`acp_console.rs::a_permission_request_appears_in_the_console_stream`).
- Posted input reaches the agent as a prompt, and input posted while a turn
  runs is queued and sent once it ends
  (`acp_console.rs::posted_input_reaches_the_agent_and_queues_behind_a_running_turn`).
- A console answer selects the option of a pending question, and takes the
  session's flag down
  (`acp_console.rs::ask_raises_attention_and_a_console_answer_unblocks_the_turn`).
- A lagged console client is told to resync, and the connection closes
  (`acp_console.rs::a_lagged_console_client_gets_a_resync_and_the_stream_ends`).
- The stored and the live events go out in id order, on the console stream
  and the terminal socket alike
  (`http/console.rs::a_stored_whole_does_not_pass_the_live_chunk_that_preceded_it`),
  and the merge keeps other sessions' events and the snapshot's last event out
  (`::the_merge_keeps_other_sessions_and_the_snapshots_last_event_out`). The
  ids are published in the order they are taken: a stored event holds the
  store's event lock from its id to its publication
  (`store.rs::an_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it`,
  `::events_written_at_once_are_published_in_id_order`), and a live event
  takes its id and goes out under the same lock
  (`acp.rs::a_live_event_that_waits_for_the_event_lock_takes_its_id_after_the_one_that_held_it`).
  A stored event reaches the bus only after the bus has loaded what its
  event carries, so a live event can still reach the merge first; the merge
  waits for the bus to publish what it was handed before the live event took
  its id, which it answers for once it has
  (`bus.rs::a_drain_answers_once_the_pump_has_published_what_it_was_handed`,
  `::a_bus_with_no_pump_answers_its_own_drain`), and the events below the
  live one then go out ahead of it, each of them once
  (`http/console.rs::a_live_event_waits_for_the_commits_the_pump_has_not_published_yet`).
  Following a turn reads the store not at all: the turn goes out whole —
  its chunks, its call and its stop — with the store shut behind the open
  (`::a_turn_follows_with_the_store_shut`).
  An event whose payload the bus cannot load is counted there, under its own
  session, where another change that fails is not
  (`bus.rs::an_agent_event_the_pump_could_not_load_is_counted_under_its_own_session`),
  and a console whose count has grown says resync before it sends a live
  event over the hole
  (`http/console.rs::an_event_the_bus_dropped_is_a_resync_rather_than_a_hole`),
  including one dropped while it waits for the bus to answer, which is when
  the events below a live one are published
  (`::an_event_dropped_while_the_console_waits_for_the_drain_is_a_resync_too`).
  A console of another session keeps its stream and its own count
  (`::a_drop_in_another_session_leaves_this_console_alone`).
  A merge whose stored channel dropped events says resync with the count of
  what it missed
  (`::a_console_that_fell_behind_on_the_stored_events_is_told_what_it_missed`),
  and one whose live channel dropped events while the console opened says
  resync before anything else
  (`::a_live_channel_that_lagged_while_the_console_opened_is_told_to_resync_first`);
  `GET /console` opens again under such a lag and refuses when it keeps
  lagging
  (`::the_snapshot_get_answers_is_opened_again_under_a_lag_and_refused_when_it_keeps_lagging`).
- A stream opened mid-turn reads the text so far in its snapshot
  (`acp_console.rs::a_stream_opened_mid_turn_gets_the_text_so_far_in_its_snapshot`),
  where the text sits by id among the stored events
  (`http/console.rs::the_text_so_far_sits_by_id_among_the_stored_events`),
  a live chunk sent between the two reads joins the snapshot in its place
  and is not sent again
  (`::a_live_chunk_sent_between_the_two_reads_joins_the_snapshot_in_its_place`),
  a live event above the snapshot's last stored event follows it once
  (`::a_live_event_above_the_snapshots_last_stored_event_follows_it_once`),
  and the chunks arrive live before the turn ends
  (`::a_console_stream_client_sees_message_chunks_before_the_turn_ends`).
- Cancel ends the running turn as `cancelled`
  (`acp_console.rs::cancelling_a_running_turn_ends_it_as_cancelled`), is
  refused between turns (`::cancel_with_no_turn_running_is_refused`), and is
  in the OpenAPI document (`::the_cancel_endpoint_is_in_the_openapi_document`).
- The terminal socket serves bytes that draw the transcript, the status
  row and the footer at the size the client sent
  (`acp_terminal.rs::the_terminal_draws_the_transcript_and_the_status_line_at_the_client_size`),
  typed keys and Enter reach the agent as a prompt
  (`::typed_keys_and_enter_reach_the_stub_agent_as_a_prompt`), a key answers
  a pending permission question
  (`::a_key_answers_a_pending_permission_question`), closing the socket
  leaves the session alive and the session ending closes the socket
  (`::closing_the_socket_leaves_the_session_alive_and_the_session_ending_closes_it`),
  a socket opened after the session ended gets the transcript and closes
  (`::a_socket_opened_after_the_session_ended_gets_the_transcript_and_closes`;
  the loop leaves on such a snapshot:
  `ariadne-console/tui/mod.rs::a_snapshot_that_holds_the_session_end_leaves_the_console`),
  a relaunch keeps an open socket open, and a later socket too
  (`::a_relaunch_keeps_the_socket_open_and_a_later_socket_too`; a snapshot
  whose session started again after its end keeps the console:
  `ariadne-console/tui/mod.rs::a_snapshot_whose_session_started_again_after_its_end_keeps_the_console`),
  Ctrl-C twice and Ctrl-D close it with the session alive
  (`::ctrl_c_twice_or_ctrl_d_closes_the_socket_and_the_session_stays_alive`),
  a resize redraws at the new size (`::a_resize_redraws_at_the_new_size`),
  an unknown session is refused before the upgrade
  (`::a_terminal_for_an_unknown_session_is_refused_before_the_upgrade`), and
  the endpoint is in the OpenAPI document
  (`::the_terminal_endpoint_is_in_the_openapi_document`).
- The ANSI backend draws what the test backend draws
  (`ariadne-console/ansi.rs::the_bytes_draw_what_the_test_backend_draws`),
  keeps a bold cell bold when the dim beside it ends
  (`::a_cell_keeps_its_bold_when_the_dim_beside_it_ends`),
  knows the cursor and the size without a query
  (`::the_cursor_and_the_size_are_known_without_a_query`), pushes inserted
  lines into the emulator's scrollback
  (`::an_inline_viewport_pushes_inserted_lines_into_the_scrollback`), and
  redraws the viewport at a new size
  (`::a_resize_redraws_the_viewport_at_the_new_size`).
- The CLI console renders a transcript and delivers an input line
  (`console.rs::a_console_renders_a_stub_agent_transcript_and_delivers_an_input_line`),
  and renders a permission question and delivers the selected answer
  (`::a_permission_question_renders_and_delivers_the_selected_answer`). Both
  are the plain protocol a redirected console keeps
  (`console/tui.rs::only_a_terminal_on_both_ends_gets_the_inline_console`).
- The inline pane renders the prompt, the agent's markdown, a tool result
  folded to its last lines with its trailing blank lines trimmed, and the
  status line
  (`ariadne-console/tui/mod.rs::a_transcript_renders_the_prompt_the_markdown_the_tool_call_and_the_status_line`),
  and markdown keeps a heading, a code block and a list apart
  (`ariadne-console/markdown.rs::a_heading_a_code_block_and_a_list_each_keep_their_own_style`,
  `::a_paragraph_wraps_at_the_width_it_is_drawn_at`).
- Markdown tables align wide cells under bold, not underlined, headers
  (`ariadne-console/markdown.rs::a_table_aligns_wide_cells_under_its_headers`),
  honour right alignment (`::a_right_aligned_table_column_is_flush_right`),
  wrap wide cells without loss (`::a_wide_table_wraps_each_cell_without_losing_text`),
  keep short columns whole and wrap long cells at spaces
  (`::a_wide_table_keeps_short_columns_whole_and_wraps_long_cells_at_spaces`,
  `::a_word_wider_than_its_cell_is_cut_not_let_out`),
  and become pairs in a narrow pane (`::a_narrow_table_draws_header_value_pairs`).
- Fenced code shows its language without a fence and wraps every character
  (`ariadne-console/markdown.rs::a_heading_a_code_block_and_a_list_each_keep_their_own_style`,
  `::a_long_code_line_wraps_with_continuation_marks_without_loss`).
- Markdown links retain a non-autolink destination once
  (`ariadne-console/markdown.rs::links_keep_their_destination_once`), lists
  show nested glyphs and task markers
  (`::nested_lists_use_a_glyph_and_indent_for_each_depth`), and quoted wraps
  retain their bars (`::every_wrapped_quote_row_keeps_its_bar`).
- Each incomplete markdown prefix renders at each supported pane width
  (`ariadne-console/markdown.rs::every_prefix_of_streamed_markdown_renders_without_a_panic`),
  and every returned line fits its width
  (`::markdown_never_returns_a_line_wider_than_its_width`).
- Each row of each state — idle, thinking, running a tool with a long name,
  reconnecting, armed, a pending question — drops whole parts at 40, 60, 80
  and 120 columns, and never cuts one
  (`ariadne-console/tui/chrome.rs::each_row_drops_whole_parts_and_never_cuts_one_at_any_width`);
  every part of every state shows at 120 columns
  (`::at_120_columns_every_part_of_every_state_shows`), and the session's
  status and the first hint still show at 40
  (`::at_40_columns_the_status_and_the_first_hint_still_show`). A tool name
  of wide characters is measured by the columns it takes
  (`::a_tool_name_of_wide_characters_is_measured_by_the_columns_it_takes`).
- The footer draws the tokens spent, compact, at its right end
  (`ariadne-console/tui/chrome.rs::the_tokens_spent_are_drawn_compact_at_the_right_end_of_the_footer`,
  `::counts_are_compact_by_the_thousand_and_the_million`), a stop that
  reports usage moves them (`::a_stop_that_reports_usage_moves_the_footer`),
  a session that spent nothing draws none
  (`::a_session_that_spent_nothing_draws_no_tokens`), and the hints are on
  the footer and not on the status row
  (`::the_hints_are_on_the_footer_and_not_on_the_status_row`).
- The status line follows the session's status from its events
  (`ariadne-console/tui/chrome.rs::the_status_line_follows_the_sessions_status_from_its_events`)
  and is not revived off an end by the events replayed under it
  (`::a_header_that_says_exited_or_failed_is_not_revived_by_the_events_replayed`),
  while a live row resumed after an earlier end follows its new launch
  (`::a_live_session_resumed_after_its_end_follows_its_new_launch_not_the_old_end`),
  counts the running turn and stops between turns
  (`::the_status_line_counts_the_running_turn_and_stops_between_turns`),
  a turn already running at attach counts from its prompt
  (`::an_attach_during_a_turn_counts_from_the_prompt_that_began_it`), a
  reconnect's replay keeps the clock of the turn still running
  (`::a_reconnect_keeps_the_clock_of_the_turn_still_running`) and gives a
  turn begun while the stream was down its own clock
  (`::a_turn_begun_while_the_stream_was_down_counts_from_its_own_prompt`),
  and a resize
  redraws the pane at the new size
  (`::a_resize_redraws_the_viewport_at_the_new_size`).
- A line of wide characters wraps at the display width, in a block
  (`ariadne-console/tui/blocks.rs::a_line_of_wide_characters_wraps_at_the_display_width`)
  and in markdown
  (`ariadne-console/markdown.rs::a_paragraph_of_wide_characters_wraps_at_the_display_width`);
  the input wraps wide characters and puts the cursor after their cells
  (`ariadne-console/tui/input.rs::wide_characters_wrap_at_display_width_and_put_the_cursor_after_their_cells`),
  an emoji sequence measured as it is drawn
  (`::the_cursor_sits_after_an_emoji_sequence_as_it_is_drawn`); and a cut
  keeps an emoji sequence whole, in a call's head
  (`::a_head_is_cut_between_whole_emoji_sequences`) and in a code line
  (`ariadne-console/markdown.rs::a_code_line_is_cut_between_whole_emoji_sequences`).
- A tool call's head is a glyph per kind and what the call is about — the
  command, the path and line, the pattern and path, the URL — never raw JSON
  (`ariadne-console/tui/blocks.rs::each_kind_of_call_draws_its_glyph_and_what_it_is_about`);
  a completed call draws its duration
  (`::a_completed_call_draws_its_duration`); and updates of one call draw one
  block (`::updates_of_one_call_draw_one_block`), because they fold into the
  open call and the last dates its end
  (`ariadne-console/transcript.rs::updates_of_one_call_fold_into_it_and_the_last_dates_its_end`);
  a call a question asks about reaches the scrollback as it ended, not as
  pending
  (`ariadne-console/tui/mod.rs::a_call_a_question_asks_about_reaches_the_scrollback_as_it_ended_not_as_pending`).
- A diff draws a file header, coloured lines and a fold count past the limit
  (`ariadne-console/tui/blocks.rs::a_diff_draws_its_file_header_its_lines_coloured_and_a_fold_count`),
  and an old text and a new text fold to hunks with context rather than every
  old line and then every new one
  (`ariadne-console/transcript.rs::an_old_and_a_new_text_fold_to_hunks_with_context`). A
  patch without file headers takes them from the entry's `path`
  (`ariadne-console/transcript.rs::a_patch_without_file_headers_takes_them_from_the_entry_path`).
- A permission question draws the call's head and its command or its diff
  above the options
  (`ariadne-console/tui/picker.rs::a_permission_question_draws_the_call_above_its_options`),
  and while it waits the picker is on the screen whatever came after it and
  however long its diff
  (`::a_pending_picker_is_on_the_screen_whatever_came_after_it_and_however_long_its_diff`).
- A typed prompt of three rows draws the bar on each row and `❯` on the
  first
  (`ariadne-console/tui/blocks.rs::a_typed_prompt_of_three_rows_draws_the_bar_on_each_row_and_the_marker_on_the_first`).
  A prompt typed during a running turn draws `queued` until its
  `user_prompt_submit`, and is then drawn once without it
  (`ariadne-console/tui/chrome.rs::a_prompt_typed_during_a_running_turn_is_queued_until_the_daemon_takes_it`).
  A daemon prompt of 40 lines draws 6 and `… 34 more lines`
  (`ariadne-console/tui/blocks.rs::a_daemon_prompt_of_forty_lines_draws_six_and_a_count_of_the_rest`).
- A call's first output row starts with `⎿ `, and its next rows and its diff
  rows start at the same column
  (`ariadne-console/tui/blocks.rs::the_output_and_the_diff_of_a_call_hang_from_its_head_at_one_column`).
- An error of 300 columns wraps in a pane of 80 with no character lost, as
  one long word
  (`ariadne-console/tui/blocks.rs::an_error_wider_than_the_pane_wraps_with_no_character_lost`)
  and as many words, the space that ends a row kept
  (`::a_multi_word_error_wider_than_the_pane_keeps_every_character`).
- A stop with `end_turn` adds no line and a cancelled turn draws `turn
  cancelled`
  (`ariadne-console/tui/blocks.rs::a_stop_at_the_end_of_a_turn_adds_no_line_and_a_cancelled_turn_says_so`),
  every stop reason reads as words
  (`ariadne-console/transcript.rs::a_stop_reads_in_words_and_an_ended_turn_is_no_block`),
  and `ariadne session logs` prints the same
  (`transcript.rs::an_ended_turn_prints_no_block_and_a_cancelled_one_says_so_in_words`).
- An unknown event `foo.bar` with summary `baz` draws `foo.bar · baz`
  (`ariadne-console/tui/blocks.rs::an_unknown_event_draws_its_kind_and_its_summary`).
- A plan with one entry of each status draws the three marks and `plan 1/3`
  (`ariadne-console/tui/blocks.rs::a_plan_draws_a_mark_per_status_and_counts_the_completed_in_its_head`),
  and a long entry wraps under its own text
  (`::a_long_plan_entry_wraps_under_its_own_text`).
- Two live blocks have one blank line between them, and the scrollback one,
  not two
  (`ariadne-console/tui/chrome.rs::two_live_blocks_have_one_blank_line_between_them_as_in_the_scrollback`).
- A tab in a tool's output takes the columns to the next tab stop
  (`ariadne-console/tui/blocks.rs::a_tab_in_a_tool_output_takes_the_columns_to_the_next_tab_stop`),
  and a call whose raw output is a structure draws its content text
  (`ariadne-console/transcript.rs::a_structured_raw_output_gives_way_to_the_content_text`).
- A prompt draws its text alone, never the system prompt
  (`ariadne-console/tui/mod.rs::a_prompt_draws_its_text_alone_and_never_the_system_prompt`);
  an event carrying only the whole prompt draws none of it
  (`::a_prompt_carrying_only_the_whole_prompt_draws_no_system_prompt`); a
  confirmation without `text` keeps what was typed
  (`::a_typed_line_confirmed_without_its_text_keeps_what_was_typed`) while
  an older daemon's own prompt takes no pending prompt's place
  (`::an_older_daemons_own_prompt_does_not_take_a_pending_prompts_place`); and a
  daemon-sourced prompt draws under its own marker
  (`::a_daemon_sourced_prompt_draws_under_its_own_marker`).
- Where the cursor position cannot be read, the pane opens from the bottom
  row
  (`ariadne-console/tui/viewport.rs::the_console_opens_at_the_bottom_when_the_cursor_position_cannot_be_read`)
  and a finished block still reaches the scrollback
  (`::a_finished_block_reaches_the_scrollback_when_the_cursor_position_cannot_be_read`).
  A terminal that answered at the open and answers nothing after — the
  terminal once the key stream reads it — is never asked again, and a
  finished block still reaches the scrollback
  (`::a_finished_block_reaches_the_scrollback_when_only_the_first_cursor_query_is_answered`).
- The pane is as tall as what it holds. Idle — before the first block, and
  after a turn whose every block, the stop included, is committed — it is
  its pinned rows
  (`ariadne-console/tui/viewport.rs::an_idle_pane_is_as_tall_as_its_pinned_rows`),
  and no blank row lies between the scrollback and the pane but the block
  separator
  (`::no_blank_row_lies_between_the_scrollback_and_the_pane_but_the_block_separator`).
  Each of the 30 lines of a block in work shows while it is written on a
  terminal of 40 rows
  (`::each_line_of_a_block_in_work_shows_while_it_is_written`). A block of
  100 lines takes the 40 rows and shows its tail, and once committed is in
  the scrollback once, in order, with every line committed before it
  (`::a_block_longer_than_the_terminal_takes_every_row_and_reaches_the_scrollback_once`),
  and the pane shrinks back to the idle height
  (`::the_pane_shrinks_back_once_its_long_block_is_committed`). A pane that
  cannot be opened again ends the console on the terminal's error, and the
  close after it does not panic
  (`::a_pane_that_cannot_be_opened_again_ends_on_the_error_and_closes_without_a_panic`),
  and the loop returns that error to its host
  (`::the_loop_returns_the_error_of_a_pane_that_cannot_be_opened_again`).
  A backend
  that answers one cursor query is asked no other through a grow, a shrink
  and a resize
  (`::no_cursor_query_follows_the_open_through_a_grow_a_shrink_and_a_resize`),
  a resize to a shorter terminal keeps the whole pane on the screen
  (`::a_resize_to_a_shorter_terminal_keeps_the_whole_pane_on_the_screen`),
  and a terminal that reads the daemon backend's bytes shows the rows the
  test backend shows after a grow and after a shrink
  (`::the_ansi_backend_shows_the_rows_the_test_backend_shows_after_a_grow_and_a_shrink`).
  A pending permission question on a terminal of 24 rows shows its question
  and every option
  (`::a_pending_question_on_a_terminal_of_24_rows_shows_its_question_and_every_option`).
- Streamed chunks append to the block already open
  (`ariadne-console/tui/mod.rs::streamed_chunks_append_to_the_agent_block_that_is_already_open`),
  a chunk that arrives after the whole of its turn is not drawn again
  (`::a_chunk_that_arrives_after_the_whole_of_its_turn_is_not_drawn_again`),
  and text after a tool call is a block of its own that the stored whole does
  not repeat (`::agent_text_after_a_tool_call_is_a_block_of_its_own`).
- A pending permission question draws a rule labelled `permission` above it
  and a rule below its options
  (`ariadne-console/tui/picker.rs::a_pending_question_draws_a_labelled_rule_above_and_a_rule_below`).
- A permission question is a picker the arrows move: the picked option alone
  starts with `❯ `
  (`ariadne-console/tui/picker.rs::a_permission_question_renders_as_a_picker_the_arrows_move`),
  and Enter posts the option it is on
  (`::enter_posts_the_permission_option_the_picker_is_on`), without consuming
  a trailing input backslash
  (`ariadne-console/tui/input.rs::permission_enter_keeps_a_trailing_backslash_for_the_input`).
- A question with a diff of 200 lines in a room of 12 rows shows the question,
  both rules and each option
  (`ariadne-console/tui/picker.rs::a_question_with_a_long_diff_shows_the_question_both_rules_and_each_option_in_twelve_rows`).
- An answered question draws `→ ` and the option chosen, and no other option
  name and no frame
  (`ariadne-console/tui/picker.rs::an_answered_question_draws_the_chosen_option_and_no_other`).
- A question of 200 columns and a long option name wrap in a pane of 80
  columns with no character lost
  (`ariadne-console/tui/picker.rs::a_question_of_200_columns_and_a_long_option_name_wrap_in_80_without_losing_a_character`).
- A typed line is posted and its pending prompt shows at once
  (`ariadne-console/tui/mod.rs::a_pending_prompt_is_on_the_screen_before_the_daemon_confirms_it`)
  and is replaced by the confirmed one
  (`::a_typed_line_is_posted_and_its_pending_prompt_is_replaced_by_the_confirmed_one`).
  Two prompts posted before the first is confirmed stay apart
  (`::a_second_prompt_typed_before_the_first_is_confirmed_keeps_both_apart`),
  and what a running turn says is drawn above a prompt typed while it ran
  (`::what_a_running_turn_says_is_drawn_above_a_prompt_typed_while_it_ran`).
  A refused prompt is said on the transcript and does not close the console
  (`::a_refused_prompt_is_said_on_the_transcript_and_does_not_close_the_console`).
- The input box draws two horizontal rules without side or corner borders
  (`ariadne-console/tui/input.rs::the_input_box_draws_two_rules_and_no_side_or_corner_border`).
- The input box starts with `❯ ` and indents each wrapped row by two columns
  (`ariadne-console/tui/input.rs::the_first_input_row_has_a_prompt_and_each_wrapped_row_has_two_spaces`).
- Two hundred input columns wrap to three rows in an 80-column pane, with the
  end and cursor visible
  (`ariadne-console/tui/input.rs::two_hundred_columns_wrap_to_three_rows_without_hiding_the_end`).
- Wide characters wrap at display width and leave the cursor in the correct
  cell
  (`ariadne-console/tui/input.rs::wide_characters_wrap_at_display_width_and_put_the_cursor_after_their_cells`);
  a cursor after a full row uses the next row's continuation cell
  (`::the_cursor_uses_the_continuation_cell_after_a_full_wrapped_row`), and
  wrapping keeps an emoji grapheme whole (`::wrapping_keeps_an_emoji_grapheme_whole`).
- Shift+Enter, Alt+Enter, Ctrl-J and backslash then Enter add a line without
  sending; the next Enter sends every line without the backslash
  (`ariadne-console/tui/input.rs::every_newline_key_adds_a_line_and_sends_only_on_the_next_enter`).
  Ctrl-J also works when encoded as a line-feed character
  (`::ctrl_j_encoded_as_a_line_feed_adds_a_line`), while Alt-J and Shift-J
  are not newline keys (`::alt_j_and_shift_j_are_not_newline_keys`).
- A backslash before the end of its line stays, and Enter sends it
  (`ariadne-console/tui/input.rs::a_backslash_before_the_line_end_stays_in_the_prompt_enter_sends`).
- Up recalls the last sent prompt and Down restores the draft
  (`ariadne-console/tui/input.rs::up_recalls_the_last_prompt_and_down_restores_the_draft`);
  Up and Down move through a wrapped draft before using history
  (`::up_and_down_move_through_a_wrapped_draft_before_history`).
- A daemon prompt never enters input history
  (`ariadne-console/tui/input.rs::a_daemon_prompt_never_enters_history`).
- An empty input shows a dim placeholder which is never sent
  (`ariadne-console/tui/input.rs::an_empty_input_shows_a_dim_placeholder_that_is_never_sent`).
- A pasted text with two line breaks is one prompt with two line breaks, and
  sends nothing until Enter
  (`ariadne-console/tui/mod.rs::a_pasted_text_is_one_prompt_with_its_line_breaks_and_sends_nothing_until_enter`);
  it goes in at the cursor, a carriage return being a line break
  (`::a_paste_goes_in_at_the_cursor_and_a_carriage_return_is_a_line_break`).
- More than four visual input rows scroll down to keep the cursor visible
  (`ariadne-console/tui/input.rs::more_than_four_visual_rows_scroll_to_keep_the_cursor_visible`).
- The line-editing keys do what the shell's do: Ctrl-A
  (`ariadne-console/tui/input.rs::ctrl_a_moves_to_the_line_start`), Ctrl-E
  (`::ctrl_e_moves_to_the_line_end`), Ctrl-U
  (`::ctrl_u_deletes_to_the_line_start`), Ctrl-K
  (`::ctrl_k_deletes_to_the_line_end`), Ctrl-W
  (`::ctrl_w_deletes_the_word_before_the_cursor`) and the Alt arrows
  (`::alt_left_and_alt_right_move_by_word`).
- Escape cancels the running turn
  (`ariadne-console/tui/mod.rs::escape_during_a_running_turn_cancels_it`), one
  Ctrl-C keeps the console and the second leaves it
  (`::one_ctrl_c_keeps_the_console_and_the_second_leaves_it`), and the pane
  opened from the bottom row runs the loop and closes like the inline one
  (`::the_console_runs_and_closes_on_the_fallback_viewport`). The terminal is
  given back on every way out
  (`console/tui.rs::the_terminal_is_given_back_on_the_normal_path_on_an_error_and_on_ctrl_c`),
  with bracketed paste off
  (`::the_terminal_is_given_back_with_bracketed_paste_off`).
- A dropped stream says so
  (`ariadne-console/tui/chrome.rs::a_dropped_stream_says_reconnecting_on_the_status_line`),
  until the next snapshot says it is back
  (`::a_dropped_frame_says_reconnecting_until_the_next_snapshot`), and its
  fresh snapshot is not printed twice
  (`::a_reconnect_redraws_the_fresh_snapshot_without_repeating_the_scrollback`).
  The loop reads any source and writes any sink: every pane test above
  drives it from an in-memory one, with no server. The CLI's source reads
  the daemon's stream as frames and dials it again when it drops
  (`console/tui.rs::the_stream_is_read_as_frames_and_dialled_again_when_it_drops`),
  ends with the error when the first dial is refused
  (`::a_first_dial_the_daemon_refuses_ends_the_stream_with_the_error`), and
  its sink posts input and cancel and says a refusal
  (`::the_sink_posts_input_and_cancel_and_says_a_refusal`).
- An attach puts its five-value welcome banner above the first block
  (`ariadne-console/tui/banner.rs::the_banner_precedes_the_first_block_and_a_snapshot_does_not_repeat_it`).
- An orchestrator banner uses the goal title and `orchestrator`
  (`ariadne-console/tui/banner.rs::orchestrator_context_uses_the_goal_title`).
- A reconnect snapshot does not add another banner
  (`ariadne-console/tui/banner.rs::the_banner_precedes_the_first_block_and_a_snapshot_does_not_repeat_it`).
- A long title cuts at `…` and an 80-column frame never exceeds 80 columns
  (`ariadne-console/tui/banner.rs::a_long_title_is_cut_inside_an_eighty_column_frame`).
- A 30-column pane has no frame
  (`ariadne-console/tui/banner.rs::a_narrow_pane_has_no_frame`).
- Missing task and repository context omits those lines
  (`ariadne-console/tui/banner.rs::missing_context_omits_its_lines`).
- One blank line separates the banner from the first block
  (`ariadne-console/tui/mod.rs::one_blank_line_separates_the_banner_from_the_first_block`).
- The whole pane — every block, the picker, a queued prompt, a resize both
  ways, a refused post, a reconnect, a cancelled turn and the session's end —
  draws at 60×20, 80×24 and 120×40: each block once in the scrollback, in
  order, one blank line before it, every row at a mark or two columns in, no
  cell of the pane left behind, and the emulator reading the ANSI backend's
  bytes shows the pane and the cursor the test backend holds
  (`ariadne-console/tui/scenario.rs::the_whole_pane_draws_at_60_by_20`,
  `::the_whole_pane_draws_at_80_by_24`, `::the_whole_pane_draws_at_120_by_40`).
- Each mark of the transcript has one meaning
  (`ariadne-console/theme.rs::one_glyph_has_one_meaning_over_the_whole_transcript`).
- The footer's arrows count the tokens alone; the keys of a question are
  words (`ariadne-console/tui/chrome.rs::the_footer_uses_its_arrows_for_the_tokens_alone`).
- The picker's frame fits a pane of 8 and of 12 columns
  (`ariadne-console/tui/picker.rs::the_frame_of_a_picker_fits_a_pane_of_8_and_of_12_columns`).
- A code span keeps its backticks in a table cell
  (`ariadne-console/markdown.rs::a_code_span_in_a_table_cell_keeps_its_backticks_as_in_text`).
- A call a stopped turn left open holds nothing back from the scrollback
  (`ariadne-console/tui/mod.rs::a_call_a_stopped_turn_left_open_holds_nothing_back_from_the_scrollback`).
- A prompt typed while a tool runs leaves the tool on the status row
  (`ariadne-console/tui/mod.rs::a_prompt_typed_while_a_tool_runs_leaves_the_tool_on_the_status_row`).
- A refused prompt is no longer queued and holds nothing back
  (`ariadne-console/tui/mod.rs::a_refused_prompt_is_no_longer_queued_and_holds_nothing_back`).
- The blank end of a row is erased, not written as spaces
  (`ariadne-console/tui/viewport.rs::the_blank_end_of_a_row_is_erased_and_not_written_as_spaces`).
- A narrower terminal keeps the blocks that were on the screen
  (`ariadne-console/tui/viewport.rs::a_narrower_terminal_keeps_the_blocks_that_were_on_the_screen`),
  and a pane on the top row is erased row by row
  (`::a_pane_on_the_top_row_is_erased_row_by_row_and_never_from_the_corner`).
- The daemon's console draws the whole transcript again once a resize
  settles, each block once
  (`ariadne-console/tui/viewport.rs::a_console_that_redraws_whole_draws_the_transcript_again_once_a_resize_settles`),
  and the CLI's never clears the scrollback
  (`::a_console_that_fits_in_place_never_clears_the_scrollback`).
- The shell comes back on the row under the last block
  (`ariadne-console/tui/viewport.rs::the_shell_comes_back_on_the_row_under_the_last_block`),
  and the CLI says the session has ended where it has
  (`ariadne-cli/commands/console/tui.rs::the_console_closing_on_the_session_end_does_not_say_the_session_still_runs`).
- The daemon terminal socket includes the task title
  (`ariadne-daemon/tests/it/acp_terminal.rs::the_terminal_draws_the_transcript_and_the_status_line_at_the_client_size`).
- The CLI reads a task title from its daemon
  (`ariadne-cli/commands/console/tui.rs::the_cli_reads_the_task_title_for_its_banner`).
- Redirected attach retains the plain line protocol
  (`ariadne-cli/commands/console/tui.rs::only_a_terminal_on_both_ends_gets_the_inline_console`).
- A readable transcript folds tool and permission pairs, keeps full text and
  renders plain output without colour
  (`transcript.rs::a_transcript_renders_one_full_block_per_item`,
  `::no_color_gives_plain_transcript_text`).
- Tool diffs retain diff colouring
  (`transcript.rs::a_tool_call_diff_uses_diff_colouring`).
- Transcript snapshots apply tail, time and kind filters
  (`console.rs::transcript_snapshot_filters_apply_to_folded_items`).
- `ariadne session logs` keeps JSON events unchanged and follows the console
  stream (`console.rs::a_transcript_log_uses_its_snapshot_for_table_and_json_output`,
  `::a_followed_log_uses_the_console_event_stream`). Chunks stream under one
  block header (`::followed_chunks_stream_text_under_one_block_header`).

## Known gap

- Protection for a user-revived session is in memory only. A daemon restart
  loses it. The user must revive the conversation again after restart.
  The runtime retains these session ids until the daemon exits.
- The launcher refuses a second live session on one seat, and no test pins
  that refusal on its own.
- No test pins the `409` a finished session gives to console input.
- A resize is a terminal limitation for the CLI's pane. The terminal moves
  its rows before the console hears of the resize, and the console makes no
  cursor query after the open (rule 23), so it cannot know where they went.
  In tmux, a taller window pulls history rows down onto the screen, and the
  pane drawn again on its old row covers them: they leave the scrollback.
  On a narrower window, a terminal that rewraps the old pane can leave the
  rows it moved above the pane in the scrollback. The desktop app's console
  draws the whole transcript again instead, which a terminal holding the
  user's shell cannot.

## Sources

`crates/ariadne-daemon/src/launcher.rs`,
`crates/ariadne-daemon/src/http/sessions.rs`,
`crates/ariadne-daemon/src/http/console.rs`,
`crates/ariadne-daemon/src/http/terminal.rs`,
`crates/ariadne-daemon/src/acp.rs`,
`crates/ariadne-cli/src/commands/attach.rs`,
`crates/ariadne-cli/src/commands/console.rs`,
`crates/ariadne-cli/src/commands/console/tui.rs`,
`crates/ariadne-cli/src/commands/transcript.rs`,
`crates/ariadne-console/src/ansi.rs`,
`crates/ariadne-console/src/markdown.rs`,
`crates/ariadne-console/src/transcript.rs`,
`crates/ariadne-console/src/theme.rs`,
`crates/ariadne-console/src/tui/mod.rs`,
`crates/ariadne-console/src/tui/banner.rs`,
`crates/ariadne-console/src/tui/blocks.rs`,
`crates/ariadne-console/src/tui/chrome.rs`,
`crates/ariadne-console/src/tui/input.rs`,
`crates/ariadne-console/src/tui/picker.rs`,
`crates/ariadne-console/src/tui/viewport.rs`.
