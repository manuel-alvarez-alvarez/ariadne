---
id: desktop-app
status: current
updated: 2026-09-12
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10, ced9f4f8, c11241f3]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/api/**/*.test.ts
  - ui/src/components/**/*.test.tsx
  - ui/src/lib/**/*.test.ts
---

# Desktop app

Ariadne Desktop: the same daemon, driven from a window. A Tauri shell around
a React app that talks to the daemon over HTTP and follows one SSE stream.

## Scope

In: the app's layout and screens, how it reaches the daemon, the query-key
and event-stream conventions, the routes and keyboard chords, and parity with
the CLI.

Out: the daemon endpoints themselves (012).

## Behavior

1. Every user-facing action exists here and in the CLI alike (014). A feature
   that lands in one is not finished until it is in the other.
2. The shell is a sidebar and a main area; a panel opens beside a list rather
   than replacing it, and the URL carries which panel is open.
3. Screens: the goals board (swimlanes plus an attention strip), the task
   panel (facts, diff, messages, history), sessions, each shown in its
   console, outside sessions that a ready task can adopt as its author,
   skills, repositories, one repository's memory (list, search, delete), the
   agents of the daemon's ACP registry with their launch flags and the models
   each may be staffed on, and a daemon-logs drawer.
4. Types are generated from the daemon's OpenAPI document, so a DTO change
   that is not reflected here fails the typecheck rather than the app.
5. One SSE connection serves the whole app, with a dispatcher and reconnect
   machinery behind it; fat events (012) are applied to the cached queries
   directly rather than triggering a re-fetch.
6. Query keys follow one convention, so an event knows which caches it
   invalidates.
7. The Tauri shell is deliberately empty — no commands — and `ui/src-tauri` is
   excluded from the cargo workspace, so a workspace build never builds the
   app.
8. On Linux, the shell sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` before the
   webview starts, unless the user already set it. WebKitGTK's DMA-BUF
   renderer aborts with `EGL_BAD_PARAMETER` on some systems; this stops that
   abort. macOS is unaffected.
9. On Linux the shell also tells GTK its own application id (`enableGTKAppId`,
   so the identifier `dev.ariadne.ui` becomes the window's), which is how a
   desktop shell matches the window on screen to the entry the installer
   wrote for it (016) — and therefore how the window gets the app's icon
   rather than a generic one.
10. The primary surface is the macOS Tauri window (WebKit): a layout change is
    verified there, not only in a browser.
11. The app is checked by `npm test`, `npm run typecheck`, `npm run lint` and
    `npm run check:unused` before a commit.
12. A goal, task author and task reviewer each name a concrete
    `<agent>:<model>` before their form can submit, where `<agent>` is a
    registry agent id (011). An empty effort stays valid and uses that
    model's default effort.
13. The model picker lists concrete catalog entries only, under one heading
    per registry agent, in the order the catalog first names each agent. No
    screen shows an automatic or default model; `auto` is an effort choice
    only.
14. The client checks only the shape of a pin: one `:`, with text on both
    sides of it. Which agents exist is the daemon's answer, so the client
    refuses no agent id by name. Only the first `:` splits the pin; a later
    one is part of the model.
15. A model the catalog lists takes only the efforts the catalog gives it. A
    model the catalog does not list takes a free-text effort, as the daemon
    takes any effort that is not blank for such a model (011).
16. The task form's skill boxes suggest only skills that can staff a task
    agent; the orchestrator's own playbook is not among them, for the author
    or a reviewer. The skills screen marks that playbook beside its built-in
    mark, staying editable and resettable like any other shipped skill.
17. The agent activity feed shows each event's one-line summary from the
    daemon; its raw payload stays available under the row.
18. The outside-sessions view lists each stored session of an ACP agent from
    `GET /v1/outside-sessions`: its registry agent id, working directory,
    last activity and first prompt. A filter bar above the table narrows it by
    agent — the registry of `GET /v1/acp-agents` — working directory, a since
    day, an until day, and a search over the first prompts. Each filter is a
    URL search param under the daemon's own name for it (`?agent=`, `?dir=`,
    `?since=`, `?until=`, `?q=`), so a narrowed screen is what its URL says
    and opens again with those filters set; a typed one reaches the daemon
    once the typing has settled rather than on every keystroke, and a day
    reaches it as the moment that bounds it — the first instant of the day as
    `since`, its finest last moment as `until` (`23:59:59.999999999Z`), in UTC,
    so a day holds every session active on it whatever precision the agent
    reported. The table takes one page at a time:
    Load more asks for the `next_cursor` the last page carried and appends
    what comes back, and one count line reads `<shown> of <total>`. Refresh
    refetches from the first page with `refresh=true`, which is what asks
    every agent again. The view offers only the ready tasks whose
    author's pin names the same agent, and adopts the session as that task's
    author through `POST /v1/tasks/{id}/author-session`, sending the agent id
    and the session id. Below the table it names every ACP agent from
    `GET /v1/acp-agents` that cannot list its sessions, with why.
19. On a task staffed with several authors (004) the task panel shows every
    one of them — its skills, its model, its own branch, and its status in the
    pick: the votes it has so far, or "Picked" once it is the one that won —
    and the reviewer pick itself: which author each reviewer chose. A task
    with one author shows the singular Author fact and no pick, unchanged.
20. Every session is shown in its console: a terminal-style pane, monospace,
    one dark surface in both themes, one column, full height when expanded.
    The console is the transcript from `GET /console` and
    `.../console/stream` (008) folded into items, an input line pinned at
    the bottom posting to `.../console/input`, and permission questions
    answered inline. `agent_message_chunk`s accumulate into one in-progress
    item that the stored `agent_message` replaces, and thought chunks do the
    same; agent text renders as markdown, and a thought renders dimmed and
    folded to two lines with a toggle. A tool call is one row — a status
    glyph (running, done, failed), its name and its input — into which each
    `tool_call_update` and the `post_tool_use` fold; it opens to the tool's
    output, and a `content` entry of type `diff` opens to a diff view. A
    `plan` renders as a checklist with one status per entry and updates in
    place. A `user_prompt_submit` is a `> text` line; a prompt whose
    `source` is `daemon` is dimmed and folded. A `stop` whose reason is
    `cancelled` reads "Stopped". Anything else the runtime reports gets the
    one-line-summary-plus-payload row of the agent activity feed. A message
    just sent is shown at once as a pending `> text` line, until the
    `user_prompt_submit` that confirms it arrives; a prompt from a console
    confirms the oldest pending line with its text, and a prompt from the
    daemon confirms none. The input takes nothing before the console's
    first snapshot, so every prompt of the history is on record before a
    line can be pending, and a reconnect's snapshot confirms only with the
    prompts it adds. Enter sends and Shift+Enter inserts a newline. A
    session that has ended takes no new message.
21. While a turn runs — between a `user_prompt_submit` and its `stop` — a
    Stop button beside the input, and Escape while the input has focus,
    post to `.../console/cancel` (008). Between turns neither is offered.
22. The pane follows new output: it stays at the bottom while output
    arrives, releases when the user scrolls up and shows a "Jump to latest"
    chip, and re-arms on a scroll back down or on the chip. The logic is
    the daemon-logs drawer's.
23. While the pane has focus and its input is empty, the number keys 1 to 9
    pick the option of that number on the oldest open permission question,
    posted as the option's label the same way a click is. The number keys
    and Escape are handled inside the pane and reach no chord of the
    shell's.
24. A session's view has two tabs over one space: the console, open by
    default, and the agent activity feed. The tab is in the URL (`?tab=`);
    the console's tab is `terminal` on the wire, so an older link still
    opens it. A tab value that is not one of the two opens the console.
    Leaving the console's tab closes its stream, and coming back opens a new
    one.
25. A session's Agent fact and the sessions list show the pin the session
    was launched on, whole (`<agent>:<model>`), with the effort after an `@`
    where one is pinned.
26. A row of the attention strip for an agent blocked on a permission or an
    input prompt opens that session's console with `?focus=`, so the console
    takes the keyboard on arrival.
27. Above the console, a session blocked on a permission or an input prompt
    shows a banner that says where to answer: the options in the console for
    a permission, the console's input for a question. A session that has
    ended while blocked is told to resume first. No other attention reason
    shows the banner.
28. The typed keyboard chords (`n`, `g` then a letter, `?`, `[`) are ignored
    while a field, an editor or a session's console input has the keyboard.
29. The agents screen has one tab per registry agent from `GET /v1/agents`,
    in the daemon's order, named by its agent id. Each tab holds that agent's
    extra flags and the models of the catalog whose `agent_id` is that agent.
    A flag edit replaces the list whole through `PUT /v1/agents/{id}`.

## Acceptance criteria

- 70 test files cover the features, the API layer and the event stream; each
  screen's behaviour is asserted in its own `*.test.tsx` beside it.
- A task staffed with several authors shows each one's branch and its own
  vote count, marks the one the reviewers picked, and lists what each
  reviewer chose; a one-author task renders as before
  (`ui/src/features/tasks/task-panel.test.tsx::shows every author's own
  branch, marking only the one the reviewers picked`,
  `::shows an author's own vote count before the pick settles`,
  `::lists what each reviewer picked, oldest first`,
  `::keeps the singular Author fact and shows no pick on a one-author task`)
  — parity with `ariadne task inspect`'s own author and picks lines (004).
- One repository's memory lists, searches through the daemon's own search
  endpoint, and deletes an entry
  (`ui/src/features/memory/memory-page.test.tsx`) — parity with `ariadne
  memory ls|search|delete` (019).
- The task's channel reads as one list, every kind is told apart, and both
  ends of a message are named by the skills they work with
  (`ui/src/features/tasks/task-messages.test.tsx`).
- Every judgement the orchestrator makes about a task can be made here too:
  how a task ends
  (`ui/src/features/tasks/task-form-dialog.test.tsx::sends the selected landing choice`),
  who reviews it
  (`ui/src/features/tasks/task-form-values.test.ts::replaces the whole reviewer list, each with its skills and its pin`),
  and whether the goal is over
  (`ui/src/features/goals/goal-actions.test.tsx::completing a goal`).
- The agents screen gives every registry agent a tab, puts each model in the
  tab of the agent that runs it, counts each agent's catalog on its tab, and
  sends a flag edit to that agent's endpoint
  (`ui/src/features/agents/agents-page.test.tsx::gives every agent a tab, and opens on the daemon's first`,
  `::puts each model in the tab of the agent that runs it`,
  `::counts each agent's catalog on its tab`,
  `::sends the whole list, added row included, to that agent's endpoint`).
- Each model has a switch, and the screen says why where the daemon refuses
  one (`ui/src/features/agents/agents-page.test.tsx::turns a model off`,
  `::says why, where the daemon refuses to turn a model off`) — the rule of
  011 read from the surface that acts on it.
- The skills screen groups the shipped skills apart from the user's own
  (`ui/src/features/skills/skills-page.test.tsx`), and offers reset for the
  first and delete for the second and never the other way round
  (`ui/src/features/skills/skill-editor.test.tsx`) — the rule of 017 read from
  the client side.
- The orchestrator's own playbook is marked beside the built-in mark
  (`ui/src/features/skills/skill-editor.test.tsx::marks the orchestrator's own
  playbook as not a task staffing choice`) and left out of the task form's
  skill suggestions, for the author and every reviewer
  (`ui/src/features/tasks/task-form-dialog.test.tsx::suggests no
  orchestrator-only skill for the author or a reviewer`).
- The repository dialog puts a placeholder refusal on the landing-briefing
  field rather than on the branch its message also names
  (`ui/src/features/repositories/repository-form-dialog.test.tsx`).
- The attention strip holds a placeholder while its lists load and survives a
  partial failure (`ui/src/features/goals/attention-strip.test.tsx`).
- Unused exports fail `npm run check:unused`.
- The goal dialog and task dialog refuse a missing model and a pin that is
  one half only
  (`ui/src/features/goals/create-goal-dialog.test.tsx::refuses a model that names no agent, before the daemon is asked`,
  `ui/src/features/tasks/task-form-dialog.test.tsx::disables create until the author and every reviewer have models`,
  `::refuses a bare agent before it sends the task`).
- The picker lists only concrete model ids, grouped by agent
  (`ui/src/features/models/pin-picker.test.tsx::offers only concrete catalog models, grouped by agent`,
  `ui/src/features/goals/create-goal-dialog.test.tsx::offers the catalog whole, grouped by the agent each model runs on`).
- A pin is checked by its shape alone and split at its first `:`
  (`ui/src/features/models/model-ref.test.ts::takes any agent and any model, one colon apart`,
  `::refuses one half on its own by showing where the other goes`,
  `::refuses a leading colon, which names no agent`,
  `::refuses a trailing colon, which names no model`,
  `::splits at the first colon`).
- A listed model offers its own efforts, and an unlisted one takes free text
  (`ui/src/features/models/pin-picker.test.tsx::offers the efforts of the pinned model, the agent's own first, and stores the pick`,
  `::takes free text for a model of a known agent the catalog does not list`,
  `::takes free text for a model nothing has discovered`).
- The agent activity feed shows the daemon's summary and opens and closes the
  raw payload under its row
  (`ui/src/features/sessions/session-activity.test.tsx`).
- The outside-sessions view lists each stored session named by its registry
  agent id, offers only the ready tasks on the same agent, adopts one with
  the agent id sent along, and shows why an ACP agent without the
  session-listing capability offers none
  (`ui/src/features/sessions/outside-sessions-page.test.tsx::lists each outside session with its agent, directory, activity, and first prompt`,
  `::offers only the ready tasks whose author runs the session's agent`,
  `::adopts a stored session, sending the registry agent id along with it`,
  `::shows why an ACP agent without the session-listing capability offers no adoption`).
- The outside-sessions view sends every filter under the daemon's own name for
  it, opens on the filters its URL carries, grows by the page the cursor names,
  asks every agent again on Refresh, and counts what is on screen out of the
  total
  (`ui/src/features/sessions/outside-sessions-page.test.tsx::sends each filter to the daemon under the name that filter has`,
  `::opens on the filters its URL carries, and asks the daemon for them`,
  `::loads the page after the cursor the daemon gave, keeping the rows above it`,
  `::asks every agent again when Refresh is pressed`,
  `::counts the sessions on screen out of every one the filters leave`).
- A session's console renders a transcript from its stream, sends typed text
  to its input endpoint and shows it pending as a `> text` line until the
  confirming prompt replaces it, inserts a newline on Shift+Enter, takes no
  message once the session has ended, and answers an inline permission
  question
  (`ui/src/features/sessions/acp-console.test.tsx::renders the transcript the stream delivers: a snapshot, then a delta`,
  `::sends typed text to the console's input endpoint, and shows it pending until confirmed`,
  `::clears a pending line whose confirming prompt arrived while the stream was down`,
  `::takes no input before the first snapshot, so a history ending on the same text confirms nothing`,
  `::inserts a newline on Shift+Enter instead of sending`,
  `::does not queue a message once the session has ended`,
  `::answers an inline permission question`,
  `::shows a permission request the daemon already answered as resolved`).
- Message chunks stream into one agent item that the stored message replaces,
  rendered as markdown with its heading and code block; a thought streams the
  same way, dimmed and folded, and its toggle opens it
  (`ui/src/features/sessions/acp-console.test.tsx::streams message chunks into one item, which the stored message then replaces`,
  `::renders a thought dimmed and folded, and the toggle opens it`).
- A tool call is one row with its status, which its updates fold into and
  which opens to its output; a failed call says so; a diff content entry
  opens to a diff view
  (`ui/src/features/sessions/acp-console.test.tsx::shows a tool call as one row with its status, and opens it to the output`,
  `::marks a failed tool call as failed`,
  `::opens a diff content entry to a diff view`).
- A plan is a checklist that updates in place
  (`ui/src/features/sessions/acp-console.test.tsx::renders a plan as a checklist and updates it in place`).
- The 1 key answers a pending permission with its first option and stays
  inside the pane, and a digit is typed once the input holds text
  (`ui/src/features/sessions/acp-console.test.tsx::answers a pending permission with its first option on the 1 key, and keeps the key in the pane`,
  `::types a digit into the input once there is text in it`).
- The Stop button and Escape post to the cancel endpoint and the resulting
  stop reads "Stopped"; between turns there is no Stop
  (`ui/src/features/sessions/acp-console.test.tsx::stops a running turn from the Stop button and from Escape in the input, and shows the stop`,
  `::offers no Stop between turns`).
- The pane follows new output, stops after the user scrolls up, and the chip
  returns it to the bottom
  (`ui/src/features/sessions/acp-console.test.tsx::follows new output until the reader scrolls up, and the chip brings it back`).
- A session's view opens on the console, keeps its tab in the URL, falls back
  to the console for a foreign tab value, and opens a new stream on each
  return to the console
  (`ui/src/features/sessions/session-detail-view.test.tsx::opens on the console, with the activity feed a tab away`,
  `::takes its tab from the URL, and puts a switch back into it`,
  `::falls back to the console for a tab that is not one of its own`).
- A session shows its pin whole
  (`ui/src/features/sessions/session-detail-view.test.tsx::shows the model the session was launched with, once`,
  `ui/src/features/sessions/sessions-page.test.tsx::says what each session runs on, without repeating the seat beside it`).
- A blocked agent's row opens its console focused
  (`ui/src/features/goals/attention-strip.test.tsx::sends a blocked agent to its console, focused`).
- The blocked banner says where to answer, says to resume an agent that is
  gone, and shows for no other reason
  (`ui/src/features/sessions/session-blocked-banner.test.tsx::says what a permission prompt is waiting for, and where to answer it`,
  `::asks for an answer when the agent asked a question`,
  `::says the agent is gone rather than pointing at the console`,
  `::stays out of the way of every other reason`).
- The typed chords stand aside for a console's input
  (`ui/src/lib/shortcuts.test.ts::is true for form fields, the console's textarea included`,
  `ui/src/components/keyboard-shortcuts-dialog.test.tsx::says what the two vocabularies are, since neither is guessable`).

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
