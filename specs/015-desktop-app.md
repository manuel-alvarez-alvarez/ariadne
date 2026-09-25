---
id: desktop-app
status: current
updated: 2026-09-24
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10, ced9f4f8, c11241f3]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/features/**/*.test.ts
  - ui/src/routes/**/*.test.tsx
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
   panel (facts, diff, messages, history), sessions — Ariadne's own and every
   outside conversation an ACP agent stored on its own, merged into one
   listing, each shown in its console — skills, repositories, the agents of
   the daemon's ACP registry with their launch flags and the models each may
   be staffed on, and a daemon-logs drawer.
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
18. The sessions screen lists both kinds of session in one table, newest
    activity first: the title, the status, the work — in one column, the
    seat a session holds as a badge, the goal it is under and the task under
    that, each of which narrows the table to its sessions when picked — the
    agent, the age and the tokens. The daemon answers the two kinds over two endpoints —
    `GET /v1/sessions` whole, for every session Ariadne started, `GET
    /v1/outside-sessions` a page at a time, for every conversation an ACP
    agent stored on its own — and the screen merges them client-side into the
    one table. Every row shows its working directory under its title — a
    task's worktree, a loose session's directory, an outside conversation's.
    An outside row leaves its status, goal and task empty. Its Agent and
    Tokens columns read as an Ariadne row's do once the daemon reports its
    model, effort and usage — the pin it last ran on, and the tokens it has
    spent, tooltip included — and, until then, name its registry agent
    instead and leave the Tokens cell blank rather than showing a zero. A
    filter bar above the table narrows the listing by kind, agent — the
    registry of `GET /v1/acp-agents` — status, seat, directory, a since day,
    an until day, and a search over the titles; `?goal=` and `?task=` narrow
    it further, as a chip above the table. The search leads the bar, with the
    count and Refresh at the other end of its row; under it each filter is
    one compact trigger naming what it filters and, once set, what it is set
    to, drawn dashed while unset. A window control beside them sets how far
    back the outside half looks: the last 7 days it opens on every visit —
    the daemon's own default, asked for with neither `since` nor `all` —
    the last 30 as a `since` bound, or all of it with `all=true`; it is its
    own `?window=` param, not remembered between visits, and an explicit
    since or until day, being the more specific ask, is sent in its place
    where one is set. The directory and the activity window open a popover —
    the window with Today, Last 7 days and Last 30 days one click each, and
    an empty day shown as "Any day" rather than as the date WebKit fills an
    empty date field with — and Clear filters drops every filter of the bar
    in one step, leaving the goal and task scope. Each filter is a URL
    search param under the daemon's own name for it (`?kind=`, `?agent=`,
    `?status=`, `?seat=`, `?dir=`, `?since=`, `?until=`, `?q=`), so a
    narrowed screen is what its URL says and opens again with those filters
    set; a typed one reaches the daemon once the typing has settled rather
    than on every keystroke, and a day reaches it as the moment that bounds
    it — the first instant of the day as `since`, its finest last moment as
    `until` (`23:59:59.999999999Z`), in UTC, so a day holds every session
    active on it whatever precision the agent reported. `status`, `seat`, `goal` and
    `task` only ever reach `GET /v1/sessions`, being things an outside
    session has none of; a goal or a task is therefore a structural
    impossibility for one, and the outside half of the screen is skipped
    rather than asked for an answer that can only be empty. `agent`, `dir`,
    `since`, `until` and `q` reach `GET /v1/outside-sessions` the way they
    always did, and narrow the Ariadne rows here, so a filter reads as one
    filter over the whole table rather than one that only works on half of
    it; a real status or a seat leaves an Ariadne row's half fetching but
    takes it out of the merged rows, since neither is something an outside
    session could match either. `status` and `seat` are remembered between
    visits, the way this screen's filters always were; the rest are not,
    since they are for finding one conversation. The Ariadne half arrives
    whole, so only the outside half pages: Load more asks for the
    `next_cursor` the last page carried and appends what comes back, and one
    count line — shown only while the outside half is part of the merged
    rows — reads `<shown> of <total>` over both halves together. Refresh
    refetches the outside half from its first page with `refresh=true`,
    which is what asks every outside agent again. Picking an Ariadne row
    opens its own panel (`?session=`) directly. Picking an outside row calls
    `POST /v1/outside-sessions/resume` once, with its registry agent id and
    internal session id, then opens the panel of the live session the daemon
    hands back — which is what turns a stored conversation into one Ariadne
    can show a console for. That session is loose: no goal, task or seat of
    its own, same as any other session the screen has none of a fact for.
    From then on it holds the conversation, so its outside row leaves every
    cached page at once and the outside half is fetched again; a
    `session_created` event does the same, for a resume made from the CLI or
    another window.
19. On a task staffed with several authors (004) the task panel shows every
    one of them — its skills, its model, its own branch, and its status in the
    pick: the votes it has so far, or "Picked" once it is the one that won —
    and the reviewer pick itself: which author each reviewer chose. A task
    with one author shows the singular Author fact and no pick, unchanged.
20. Every session is shown in its console, as the CLI draws it: a terminal
    emulator (xterm.js) on the daemon's terminal socket
    (`GET /v1/sessions/{id}/console/terminal`, 008), in which the daemon
    runs the console of 008's rules 22 to 29 itself. The pane sends its
    size before anything else — the daemon draws nothing until it has one
    — and again on every refit, writes every binary frame into the
    emulator as it is, and the emulator's scrollback is the transcript's
    history. What the input box, Escape, the permission picker and the
    line-editing keys do is the console's own, the same for the CLI and
    for the pane. Every open resets the emulator first, since each
    connection draws the console whole. An Expand control opens the console
    in a near-fullscreen modal, and its Collapse control restores it to the
    panel. Each move closes the old socket before it opens a fresh socket,
    refits the emulator, and sends the new size before input. Focused Escape
    remains console input; Escape outside the terminal closes the modal.
21. Every key press is sent as a `key` message, the DOM key mapped one to
    one onto crossterm's code and modifiers — a printable character as
    itself, the named keys by name, F1 to F12 by number, Shift+Tab as
    `back_tab` — and every paste as a `paste` with its text; xterm.js's own
    key handling is bypassed. Three things are left to the browser rather
    than sent: a bare modifier, a chord held with the command key on
    macOS, and Ctrl+Shift+C and Ctrl+Shift+V, so copying the selection and
    the paste event that becomes the `paste` keep working. Option on macOS
    reads the letter off the physical key, so Alt-B is a word left there
    too. Keys typed before the socket is open go nowhere.
22. The pane takes the app's colours and font from the tokens the rest of
    the UI is drawn in, in both themes, re-read when the theme switches:
    the six named terminal colours map onto the status ramp. It fills the
    box it is given, refits when that box changes, and the daemon redraws
    at the new size. It draws on WebGL where the webview has a context, so
    box-drawing glyphs — the input box's rules, a table's rule — are the
    emulator's own and join into one line; without one it keeps the DOM
    renderer, which takes them from the font.
23. A drop of the socket is retried on the event stream's backoff, and the
    pane says it is reconnecting meanwhile. A close the daemon meant — the
    session ended, Ctrl-C twice, Ctrl-D — ends the console instead: the
    pane says the console closed, or that the session ended when the
    daemon's last status frame said so, and a Reopen button opens another.
24. A session's view has two tabs over one space: the console, open by
    default, and the agent activity feed. The tab is in the URL (`?tab=`);
    the console's tab is `terminal` on the wire, so an older link still
    opens it. A tab value that is not one of the two opens the console.
    Leaving the console's tab closes its socket, and coming back opens a
    new one.
25. A session's Agent fact and the sessions list show the pin the session
    was launched on, whole (`<agent>:<model>`), with the effort after an `@`
    where one is pinned.
26. A row of the attention strip for an agent blocked on a permission or an
    input prompt opens that session's console with `?focus=`, so the terminal
    takes the keyboard on arrival.
27. Above the console, a session blocked on a permission or an input prompt
    shows a banner that says where to answer: the picker in the console for
    a permission, the console's input for a question. A session that has
    ended while blocked is told to resume first. No other attention reason
    shows the banner.
28. The typed keyboard chords (`n`, `g` then a letter, `?`, `[`) are ignored
    while a field, an editor or a session's terminal has the keyboard.
29. The agents screen has one tab per registry agent from `GET /v1/agents`,
    in the daemon's order, named by its agent id. Each tab holds that agent's
    extra flags and the models of the catalog whose `agent_id` is that agent.
    A flag edit replaces the list whole through `PUT /v1/agents/{id}`. A
    Refresh control above the tabs calls `POST /v1/acp-agents/refresh` once,
    which reprobes every registry agent and picks up one installed since the
    daemon started; on its answer the agent configs, the ACP agents and the
    models are all reloaded, since a rediscovered agent can move any of the
    three. The control shows a pending state while the call runs, and a
    failed call is toasted, leaving the screen as it was.
30. A session panel shows a reported context window as `<used> / <size>`,
    using the compact spelling of token figures. It shows no context fact
    before the agent reports one, and it never shows a cost.
31. Beside each model's switch, a picker shows its `rank` from `GET
    /v1/models`: `frontier`, `balanced`, `fast`, `local`, or unranked where it
    is `null`. Picking one of the four, or Unranked to clear it, sends `PUT
    /v1/models/rank` with the model's id and the lowercase rank word (or
    `null`) in the body — never the id in the path, since a model id carries
    `:` and, for opencode's, `/`. The picker's own list says what each rank
    means, in one line apiece: the orchestrator staffs the smallest ranked
    model a task earns, with `local` off that ladder — staffed only where a
    task names it — and an unranked model still usable. A write patches the
    row from the daemon's answer through the same `models` query key a switch
    write uses, and a refusal is toasted rather than swallowed, springing the
    picker back to what the daemon still says.

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
- The sidebar lists Repositories last, right after Agents, and `#/repositories`
  mounts the repositories screen, while a screen the app dropped leads nowhere
  (`ui/src/components/app-shell.test.tsx::ends the navigation with repositories, right after agents`,
  `ui/src/routes/router.test.tsx::mounts the repositories screen at #/repositories`,
  `ui/src/routes/router.test.tsx::leads nowhere from a screen the app no longer has`).
- The palette opens `#/repositories` on a picked repository
  (`ui/src/features/command-palette/command-palette.test.tsx::opens the repositories screen on a repository from the palette`).
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
- Refresh calls the reprobe endpoint once, reloads the agent configs, the ACP
  agents and the models, shows a pending state while it runs, and a failed
  call leaves every tab as it was
  (`ui/src/features/agents/agents-page.test.tsx::posts to the refresh
  endpoint once, and reloads the configs, the ACP agents and the models`,
  `::shows a pending state while the call is running`,
  `::shows an error on a failed call, and keeps the screen as it was`).
- Each model's rank picker shows its rank or Unranked, names what each of the
  four ranks means in its own list, sends each pick as its lowercase word and
  a clear as `null` with the model's id in the body, refreshes through the
  models query key, and the screen says why where the daemon refuses a change
  (`ui/src/features/agents/agents-page.test.tsx::shows the rank of a ranked
  model, and an unranked model as unranked`,
  `::says what each rank means, in the picker's own list`,
  `::sends each rank as its lowercase word, with the id in the body`,
  `::sends null to clear a rank back to unranked`,
  `::refreshes the catalog through the models query key after a change`,
  `::says why, where the daemon refuses to rank a model`).
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
- An attention toast opens the blocked session and finishes dismissal before
  the test removes its browser environment
  (`ui/src/features/goals/attention-alerts.test.tsx::raises one toast for an agent that gets stuck on another screen`).
- Unused declared dependencies, exports only tests import, and orphan source files fail `npm run check:unused`.
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
- A session panel shows its reported context window with compact token figures
  and hides an unreported one
  (`ui/src/features/sessions/session-detail-view.test.tsx::shows the reported context window with compact token figures`,
  `::hides context when the agent has not reported a window`).
- One table lists Ariadne sessions and outside sessions together, newest
  activity first, and an outside row's empty status, goal and task, naming
  its agent and its directory instead
  (`ui/src/features/sessions/sessions-page.test.tsx::lists Ariadne sessions and outside sessions together, newest activity first`,
  `::shows an outside row's empty status, goal and task, and names its agent and directory`).
- The window control opens on the last 7 days asking the daemon for nothing
  extra, sends a `since` 30 days back when that window is picked, and sends
  `all=true` for all of it; an outside row shows its model and its tokens
  once the daemon reports them, and neither, with no zero, until then
  (`ui/src/features/sessions/sessions-page.test.tsx::opens the window picker on the last 7 days, asking the daemon for nothing extra`,
  `::asks the daemon with a since bound 30 days back when that window is picked`,
  `::asks the daemon for every outside conversation when All time is picked`,
  `::shows an outside row's model and tokens once the daemon reports them`,
  `::shows neither a model nor a token figure, and no zero, on an outside row without them`).
- Every filter — kind, agent, status, seat, a day's activity window and a
  search over the titles — reaches the daemon under its own name, on the
  endpoint that takes it, and a day is sent as the moments that bound it, in
  UTC
  (`ui/src/features/sessions/sessions-page.test.tsx::sends each filter to the daemon under the name that filter has, on the endpoint that takes it`,
  `::sends a day's activity window as the moments that bound it, in UTC`).
- A preset activity window is sent as the day it starts on and named on its
  trigger; Clear filters drops every filter of the bar at once, the search
  field and the remembered status included, and keeps the scope
  (`ui/src/features/sessions/sessions-page.test.tsx::sends a preset activity window as the day it starts on, and names it on the trigger`,
  `::clears every filter of the bar at once, the search field included, and keeps the scope`).
- A preset activity window is sent as the day it starts on and named on its
  trigger; Clear filters drops every filter of the bar at once, the search
  field and the remembered status included, and keeps the scope
  (`ui/src/features/sessions/sessions-page.test.tsx::sends a preset activity window as the day it starts on, and names it on the trigger`,
  `::clears every filter of the bar at once, the search field included, and keeps the scope`).
- The outside half pages through `next_cursor`, keeping the Ariadne rows
  already shown, and counts both halves together out of the total; Refresh
  asks every outside agent again
  (`ui/src/features/sessions/sessions-page.test.tsx::pages the outside half through next_cursor, keeping the Ariadne rows, and counts the total`,
  `::asks every outside agent again when Refresh is pressed`).
- Picking an Ariadne row opens its panel directly, asking the resume endpoint
  for nothing; picking an outside row resumes it once, then opens the console
  of the session the daemon answers
  (`ui/src/features/sessions/sessions-page.test.tsx::opens an Ariadne row's own panel directly, asking the resume endpoint for nothing`,
  `::resumes an outside row once, then opens the console of the session it answers`).
- Every row shows where its agent runs under its title, and a resumed outside
  conversation is listed once, as the session that holds it, with its
  directory; a created session refetches the outside half, an updated one
  does not
  (`ui/src/features/sessions/sessions-page.test.tsx::shows where a task's agent runs under its title, as it does for every row`,
  `::shows the seat, the goal and the task of an Ariadne row together, in one Work column`,
  `::narrows the table to a goal picked in the Work column, opening no panel`,
  `::shows a resumed outside row once, as the session that holds it, with its directory`,
  `ui/src/events/dispatch.test.ts::refetches the outside lists when a session is created, since it may hold one of their rows`,
  `::leaves the outside lists alone when a session only moves on`).
- A goal chip narrows the screen and the daemon's own list alike, skips the
  outside half, clears from the chip, and the status and seat filters are
  what the screen is opened with next
  (`ui/src/features/sessions/sessions-page.test.tsx::narrows to one goal from a scope chip, skipping the outside half, and clears it`,
  `::comes back to the status and seat filters the screen was left with`).
- The outside-sessions page, its route and the adoption dialog are gone
  (`ui/src/routes/router.test.tsx::leads nowhere from the outside-sessions screen the merge dropped`).
- An unorchestrated goal names no orchestrator in its panel
  (`ui/src/features/goals/goal-panel.test.tsx::says an unorchestrated goal has no orchestrator`).
- The terminal pane sends its size before anything else, and nothing typed
  before the socket is open
  (`ui/src/features/sessions/session-terminal.test.tsx::sends its size before anything else`).
- The terminal opens in a near-fullscreen modal, closes its previous socket
  before its modal socket opens and resizes, restores a fresh panel socket on
  collapse, and gives terminal Escape priority only in the modal while outside
  Escape dismisses the modal and focused panel Escape closes its panel
  (`ui/src/features/sessions/session-terminal.test.tsx::expands the console into a near-fullscreen modal`,
  `::closes the panel socket before opening and resizing the modal console`,
  `::collapses the modal console into the panel on a fresh socket`,
  `::keeps the modal open when focused Escape belongs to the console`,
  `::closes the modal when Escape occurs outside the console`,
  `ui/src/features/sessions/session-panel.test.tsx::closes the panel when focused Escape reaches its console`).
- The bytes of a binary frame appear in the terminal
  (`ui/src/features/sessions/session-terminal.test.tsx::writes the bytes of a binary frame into the terminal`).
- A key press is sent as a `key` message with its code and modifiers — a
  character, Ctrl+Enter, Shift+Tab, Option-B, F5 — a bare modifier and a
  command-key chord are not, and a paste is sent as a `paste`
  (`ui/src/features/sessions/session-terminal.test.tsx::sends a key press as a key message, and a paste as a paste message`).
- A dropped socket says reconnecting, dials again within the backoff, and
  the new connection opens with the size again
  (`ui/src/features/sessions/session-terminal.test.tsx::says it is reconnecting after a drop, and dials again`).
- A close the daemon meant ends the console rather than retrying, says the
  session ended when the last status frame said so, and Reopen dials again
  (`ui/src/features/sessions/session-terminal.test.tsx::ends the console on a close the daemon meant, and a button opens another`).
- A session's view opens on the console, keeps its tab in the URL, falls back
  to the console for a foreign tab value, and opens a new socket on each
  return to the console
  (`ui/src/features/sessions/session-detail-view.test.tsx::opens on the console, with the activity feed a tab away`,
  `::takes its tab from the URL, and puts a switch back into it`,
  `::falls back to the console for a tab that is not one of its own`).
- A session shows its pin whole
  (`ui/src/features/sessions/session-detail-view.test.tsx::shows the model the session was launched with, once`).
- A blocked agent's row opens its console focused
  (`ui/src/features/goals/attention-strip.test.tsx::sends a blocked agent to its console, focused`).
- The blocked banner says where to answer, says to resume an agent that is
  gone, and shows for no other reason
  (`ui/src/features/sessions/session-blocked-banner.test.tsx::says what a permission prompt is waiting for, and where to answer it`,
  `::asks for an answer when the agent asked a question`,
  `::says the agent is gone rather than pointing at the console`,
  `::stays out of the way of every other reason`).
- The typed chords stand aside for a session's terminal, which types through
  a textarea
  (`ui/src/lib/shortcuts.test.ts::is true for form fields, the console's textarea included`,
  `ui/src/components/keyboard-shortcuts-dialog.test.tsx::says what the two vocabularies are, since neither is guessable`).

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
