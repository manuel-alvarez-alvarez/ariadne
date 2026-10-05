---
id: desktop-app
status: current
updated: 2026-10-05
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
2. The shell is a sidebar, ending in the daemon connection status, and a main
   area under one header bar — the screen's name as its only `h1`, and a
   screen's own actions at the header's end. Goal, task and session details
   occupy one pane that floats over the screen at its right edge. The URL
   selects its contents. The screen keeps its full width and layout behind
   the pane, under a scrim. A click on the scrim closes the pane, the same
   as the close button, and Tab stays inside the pane while it is open. The
   pane slides in from the right and the scrim fades in, unless the system
   asks for reduced motion. The close button sits in the pane's header, as
   its first tab stop. The left handle resizes the pane by pointer or
   keyboard between 24rem and 60% of the window. A drag saves the width
   once, on release, and a double-click resets it to the 36rem default;
   settings preserve the choice.
   Below `md`, the pane covers the screen at full width. The pane holds one
   panel at a time: a task opened from a goal replaces the goal's panel
   rather than stacking on it, carrying a breadcrumb back that reopens the
   goal in its place. Session drill-downs use that pane too. Escape inside
   it, or its own close control, closes the pane outright — never back to a
   goal a task replaced, even from a URL that names both. Closing preserves
   the existing history and, where an opener is still on the screen, focus
   return. The header does not shrink, and the body scrolls within the
   remaining height, with one full-height child for the session view.
3. Screens: the goals board (swimlanes plus an attention strip) has Active,
   All and Finished status segments. A status-menu icon holds a custom
   selection. Each lane header shows a progress bar, done/total and tokens.
   Its title hint carries the created stamp. The bar and a collapsed lane
   retain the lane summary. The task
   panel (facts, diff, messages, history), sessions — Ariadne's own and every
   outside conversation an ACP agent stored on its own, merged into one
   listing, each shown in its console — skills, repositories, the agents of
   the daemon's ACP registry with their launch flags and the models each may
   be staffed on, Permissions — the AI permission model's settings behind the
   `ai` permission mode (022) — Stats (rule 36), and a daemon-logs drawer.
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
   On macOS, the shell disables WKWebView's automatic quote and dash
   substitution before the webview starts, so code and structured data stay
   unchanged. WebKitGTK does not compile automatic text replacement, so Linux
   has no equivalent setting.
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
17. The agent activity feed shows each event by its kind in plain words: Tool
    call, Tool result, Permission asked, Permission answered, Agent said, and
    so on for every kind the daemon records; an unknown kind shows its raw name.
    A colored dot from the status ramp — warn for permission events, danger for
    errors, active for agent messages, pending for tool calls — precedes the
    plain words. The summary is the daemon's one-line gist, or derived from the
    payload when empty: the tool name and its first argument, showing `Read
    /tmp/x.png` or `Bash git status`. Consecutive rows of the same kind and the
    same tool fold into one, such as `Tool call · Read /file ×7`, which expands
    to the individual rows. Each row still expands to its raw payload.
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
    since they are for finding one conversation. The Ariadne half renders as
    soon as it answers; while the outside half is still loading, its rows
    stay visible with one trailing loading row, and the count reads `<n>
    sessions · looking for outside conversations`. The Ariadne half arrives
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
19. The goal, task and session panels, and the session drill-downs inside the
    goal's and the task's own, share one header component: an optional
    breadcrumb row back to the entity this one is drilled into (a goal, for a
    task's or a session's own panel; a task, for one of its sessions), with
    the focus ring every other control in the app wears; the title row, the
    title truncating to one line with the full text as its `title` attribute
    and the panel's own actions at the row's end; and the dense meta row,
    the entity's status badge first, then its id, then when it was made and
    when it last moved. A goal opened straight from the board carries no
    breadcrumb, nor does a session opened on its own. A session drill-down's
    breadcrumb replaces the "Back to …" button it used to be. The loading and
    the error state of a panel render inside its scrolling body, under a
    header that names the entity only where its data already has, so a
    failed refetch of a goal already cached shows that goal, under its one
    title, with one inline notice above it rather than a second title and a
    lost panel. The goal, task and session panels open on a dense fact list
    above their tabs — `text-xs`, three columns at `sm` and four at `lg`, no
    card frame — rather than the framed grid further down an entity's own
    screen. On a task staffed with several authors (004) the task panel shows
    every one of them — its skills, its model, its own branch, and its status
    in the pick: the votes it has so far, or "Picked" once it is the one that
    won — and the reviewer pick itself: which author each reviewer chose. A
    task with one author shows the singular Author fact and no pick,
    unchanged.
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
25. The sessions list shows the pin the session was launched on
    (`<agent>:<model>`): the row cuts the model id in the middle when needed,
    keeps the effort after an `@` readable, and its hint holds the whole pin
    (`ui/src/features/sessions/sessions-list.test.tsx::keeps the effort beside
    a middle-cut model and gives the row a whole-pin hint`).
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
    Refresh control beside the tabs calls `POST /v1/acp-agents/refresh` once,
    which reprobes every registry agent and picks up one installed since the
    daemon started; on its answer the agent configs, the ACP agents and the
    models are all reloaded, since a rediscovered agent can move any of the
    three. The control shows a pending state while the call runs, and a
    failed call is toasted, leaving the screen as it was. Its tooltip also
    says how many models are turned off; each tab keeps its own model count.
30. A session panel shows a reported context window as `<used> / <size>`,
    using the compact spelling of token figures, beside a 4rem meter of how
    much of it is used. It shows no context fact before the agent reports
    one, and it never shows a cost.
31. Every session outside a cancelled goal offers Switch beside its session
    actions. Its dialog starts on the session's pin, checks the pin's
    `<agent>:<model>` shape, and posts its model and optional effort to
    `POST /v1/sessions/{id}/switch`. A refusal shows the daemon's message; a
    successor id replaces the open session while a same id leaves it selected.
    A switched session links to the session it continues
    (`ui/src/features/sessions/session-actions.test.tsx`,
    `ui/src/features/sessions/session-detail-view.test.tsx`).
32. Beside each model's switch, a picker shows its `rank` from `GET
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
33. The Permissions screen's AI tab holds one card, "AI", with an alert at the
    top of the card for the last error, then a header that is one row: the
    title, a state badge — `disabled`, `installing`, `ready` or `failed` —
    the enabled switch, and a joined button group holding "Test a request"
    and "Refresh" as icon-only buttons, both `variant="outline"` and
    `size="icon"`, each with a tooltip and an `aria-label` carrying its full
    text, Refresh disabled while the model is off or already installing and
    showing a pending state while it runs, the group wrapping below the
    title row on a narrow window. The card's old description line is gone.
    Below the header, a Python reason line sits directly under it where the
    daemon has no Python new enough to install into; the switch itself,
    labelled "Enable the AI permission model" by its own `aria-label` rather
    than by visible text, is disabled the same way. A Thresholds section
    holds the allow and deny thresholds as one range control, a two-handle
    slider from 0 to 1 in steps of 0.01 whose track is cut into a green Allow
    zone, an amber Ask zone and a red Deny zone the handles cannot cross,
    with the zone names Allow, Ask and Deny in a row above the track,
    centered on each zone and hidden where its own zone is too narrow, and
    the two number inputs in one row below the track, never cut at the
    card's edge — "Allow threshold" ("Allow up to") with a green dot at its
    start and "Deny threshold" ("Deny from") with a red dot at its end, each shown to four fixed decimals,
    step 0.0001 — that stay in step with the handles while either is dragged
    or typed into, the digits settling to four decimals on mount, after a
    drag and after a commit, and left alone while typed into. A Model
    section holds the Flavour and Device selects side by side across the
    card's width; each option names the flavour or device, a line on what it
    is, and a line on where it runs or why it cannot, the way the rank
    picker's options carry their meaning. A Status and hardware section
    lays its facts out in the card as a grid, with no "Details" popover:
    what runs (flavour on device), the memory, the GPU, the machine, the
    Python version, the weights, the last refresh's age, the endpoint, and
    the installed and latest release. The Thresholds, Model, and Status and
    hardware headings use sentence case, medium small text, and a rule below;
    the status facts use four columns at `lg`. Every control sends its own change the
    moment it is made, there is no Save button, and a handle sends once, on
    release, never on every drag step — and a refusal is toasted with the
    daemon's own message and puts the control back to the row the daemon
    still holds, the same as the agents screen's flag editor and rank
    picker. The `ai_permissions_updated` event (012, 022) patches the same
    query key any of those writes does, since the row has no list beside it
    (`ui/src/features/permissions/permissions-page.test.tsx::puts the title,
    the badge, the switch and both icon buttons in one header row`, `::drops
    the old description line under the title`, `::shows the last error as an
    alert at the top of the card`, `::shows the Thresholds, Model and Status
    and hardware section headings`, `::joins Test a request and Refresh in
    one button group in the card header`, `::opens the test dialog from the
    header's Test a request button`, `::the model pickers > puts Flavour and
    Device in one inline row`, `::shows what runs, the memory and the GPU as
    facts`, `::shows every fact the daemon answered with, with no Details
    button`, `::the state badge > shows Disabled for the
    disabled state`, `::the state badge > shows Installing for the
    installing state`, `::the state badge > shows Ready for the ready
    state`, `::the state badge > shows Failed for the failed state`,
    `ui/src/features/permissions/ai-card.test.tsx::puts Test a request and
    Refresh in one button group in the header`, `::renders the header
    buttons icon-only, with a tooltip naming each`, `::shows the status and
    hardware facts in the card, with no Details popover`,
    `ui/src/events/dispatch.test.ts::replaces the cached status whole, so a
    card that read installing reads ready`). The repository dialog's
    `PERMISSION_MODES` gains `ai`, and an `ai_disabled` refusal on it lands on
    the permission-mode field, naming the Permissions screen rather than the
    daemon's own CLI-flavoured words
    (`ui/src/features/repositories/repository-form-dialog.test.tsx::puts an
    ai_disabled refusal on the permission mode field, pointing at the
    Permissions screen`).
34. The Permissions screen's Learned tab lists every learned approval
    (`GET /v1/permissions/learned`, filtered by `?repository=` when the URL
    carries one), its actions column pinned to the trailing edge the way
    `models/model-table.tsx`'s Available column is: the repository by its
    folder name — the last path segment, a `title` of the full path on the
    cell — the tool name and the kind each cut with an ellipsis and a `title`
    of the full value, a Request column after Kind reading the `command` of a
    shell call or the `file_path` or `path` of a file call from the row's own
    `tool_call.rawInput` — where the daemon keeps the agent's own arguments
    (`crates/ariadne-daemon/src/acp.rs`), never on the call itself — else the
    call compacted to one line, cut with an ellipsis and a `title` of the full
    text, the source (`console` or `manual`), the level, family, normalized
    key and scope beside the tool. The key is one line with an ellipsis and a
    `title` of its full value, the AI
    label and danger where the model scored the row, and how long ago it was
    learned. A row is a Tab stop that opens its detail on Enter or Space as
    well as a click, under an accessible name of its tool and its repository's
    folder. The repository filter's trigger and its options show the same
    folder name, each option's full path underneath it in muted text and on
    the trigger as a `title`; the popup is wide enough for a full path rather
    than the trigger's own width, and a `?repository=` id the registry does
    not carry still shows a readable label instead of an empty trigger. A
    filtered empty state offers Clear filter, which drops `?repository=`. The
    tab shows no row count above the table. Add
    approval opens a form for the repository, the tool name and the kind
    (`POST /v1/permissions/learned`), whose own repository picker follows the
    same folder-name-with-path convention; each row's Edit changes the tool
    name and the kind (`PUT .../{id}`) and Delete removes it
    (`DELETE .../{id}`) after a confirm naming the repository's folder and the
    row's own request rather than only the tool name, and a refusal from any
    of the three is toasted with the daemon's own message rather than held on
    the dialog. A row opens a detail panel with every field, the repository's
    full path as a `title` on its cut fact, its level, family, normalized key,
    risk tags and scope, the stored `tool_call` and
    `options` pretty-printed for a console row (a manual row says none was
    recorded), the selected option, links to the session and the task where
    the row carries one, and, where the model scored the row, the AI label,
    both probabilities, the danger, both thresholds, the cap and the risk
    tags — the same facts the row's own AI score column carries, in full. Its
    Scope control selects This repository or All repositories and sends only
    `scope` to `PUT /v1/permissions/learned/{id}`. A refusal toasts the
    daemon's message and restores the stored scope.
    Which tab is
    open lives in the URL the way every other panel's does — `?tab=learned` or
    `?tab=ai`, Learned by default, since it is where every mode leaves what it
    decided — and the repository filter lives beside it as `?repository=`
    (`ui/src/features/permissions/permissions-page.test.tsx::opens the Learned
    tab when the URL says nothing`, `::puts the picked tab on the URL`,
    `ui/src/features/permissions/learned-tab.test.tsx::shows a row's tool
    name, target, selected option outcome, created and updated`,
    `::shows each row's level, family, key and scope, cutting a long key to
    one line`, `::shows every field, with the tool call, the options and a
    null output as JSON`, `::sends only scope when widening a row to all
    repositories`, `::toasts a scope refusal and restores the saved value`,
    `::shows the output's label, both probabilities, danger, both thresholds
    and the tags where the model was called`,
    `ui/src/events/dispatch.test.ts::patches created and updated details and
    refetches lists`).
35. The AI tab's card carries a "Test a request" button in its header button
    group, beside Refresh.
    It opens a wide dialog for `POST /v1/permissions/ai/test`: Title, Kind,
    Input (JSON, at least six rows), Options (one name per line), Locations
    (one path per line) and Workspace fields. Title is the tool call title the
    model sees, such as the command, `Edit <path>` or `Fetch <url>`, never the
    tool name, and its help text says so. An empty Locations or Workspace sends
    `null`; a workspace gives the daemon the root for deriving whether a path
    is outside it. The dialog is prefilled with the Run the tests example:
    title `npm test`, kind `execute`, a `command` with a `description`, the
    options `Yes`, `Yes, and don't ask again for npm test * commands` and
    `No`, and the workspace `/Users/user/.ariadne/worktrees/goal/task`. Only
    the fields scroll on a short window, with focus-ring padding;
    the header and footer stay in view and no width shows a horizontal scrollbar.
    Input is pretty-printed JSON and resizes vertically only. The title row
    holds an Examples picker beside the title, clear of the close button, that
    fills all six fields from ten cases in the shape a real `claude-acp`
    request has: Run the tests, Edit a file in the worktree, Read a file
    outside the worktree, Write the shell startup file, Fetch a page, Search
    the web, Read the task messages, Finish the task, Pipe a script to the
    shell and Send SSH keys to a paste site. Each carries the three real
    option names, the input path in Locations where a real request has one,
    and the generic workspace. Test is the dialog's primary button and Cmd/Ctrl+Enter runs it. Test
    sends exactly what the fields hold, is disabled while the JSON does not
    parse (a field error says so) or the model is off, and shows a pending state
    while it runs. The footer holds the polite result at the left of Test: a
    badge in its zone's own colour — green Allow, amber Ask, red Deny — the
    allow and deny probabilities out of `probabilities`, to two decimals, and the
    labelled danger to four decimals with tabular digits. Where the daemon derives
    them, the result also shows the operation and one badge for each risk tag. A
    capped result shows `capped by <tag>` beside its Ask badge and danger. The same danger also
    becomes the range control's marker (its `danger` prop, 32) after the dialog
    closes. The label is never the response's own: it is worked out here from
    that danger and the thresholds currently shown, by rule 28 of 022 (at or
    under allow is `allow`, at or over deny is `deny`, between is `ask`), so a
    threshold dragged or typed afterwards relabels the same score with no second
    call. An `ai_error` answer shows "No answer: <ai_error>" in the footer,
    wraps without moving Test out of the dialog, and draws no marker. The footer
    also shows the model-off hint. A refusal is toasted with the daemon's own
    message (`ui/src/features/permissions/ai-test-panel.test.tsx::opens
    prefilled with the npm test example, in the shape a real request has`,
    `::the example picker > sits in the header and offers the ten examples`,
    `::keeps the fields in a scroll region between the header and
    footer`, `::uses a wide dialog and lets Input resize vertically`,
    `::the example picker > fills every field of the %s example`, `::sends
    the example's locations, options and workspace`, `::sends a typed title,
    kind, options and locations one per line, edited by hand`,
    `::runs the test when Cmd or Ctrl+Enter is pressed`, `::shows the label and danger in the polite footer
    result`, `::shows the allow and deny probabilities in the footer result`,
    `::shows the operation and each risk tag in the footer result`,
    `::shows the cap beside an ask
    result`, `::keeps the existing result when no derived facts are present`,
    `::shows 'No answer: <ai_error>' in the footer, with no
    label`, `::shows a field error on invalid JSON and disables Test`,
    `::shows the model-off hint in the footer and disables Test`, `::toasts the
    daemon's own message on a refusal`,
    `ui/src/features/permissions/ai-card.test.tsx::draws the marker and the
    label from a test result`, `::relabels the same danger once a threshold
    moves, with no second call`, `::draws no marker for an ai_error answer`).
36. The Stats screen is at `#/stats`, titled `Stats`, last in the sidebar
    (023). Its header holds a `since` selector — all time, 24 hours, 7 days,
    30 days — and a repository selector, both kept in the URL as `?since=`
    and `?repo=`. The screen is a dashboard. It leads with a `Key figures`
    row: tasks finished, finish rate and median goal lead time off the work
    family, total tokens (input and output) off the spend family, and
    interventions and person time, the models family's `interventions.total`
    and `interventions.person_secs` summed over its rows; a figure reads `—`
    until its answer is in. Under it, the screen passes `{ since, repo }` to
    five sections, in order: Models, Work, Time, Spend and Attention, each
    `src/components/stats/<family>-section.tsx`, laid out as a card grid —
    one column, and two from `xl` up, where Models spans both. Every section
    draws through the shared `StatSection`, a card: its heading,
    `text-sm font-medium`, the one sentence of the question it answers, the
    read's error or skeleton, and the one muted sentence of an empty family.
    Cards in one grid row share its height; a chart is at least 160px high
    and grows into the room its card leaves, and tiles wrap to fill each of
    their rows. Each reads
    `GET /v1/stats/<family>` under `qk.stats.<family>(filter)`, and every
    `task_updated` and `session_updated` event invalidates the `stats` group
    — parity with `ariadne stats <family>` (014). A chart is colour-coded by
    meaning off the status ramp through the `STATUS_COLORS` module, and backed
    by an `sr-only` table of the same numbers for a screen reader; that
    table's wrapper carries `relative` so the table's own `absolute`
    positioning stays inside it rather than the document, which would
    otherwise stretch the page past the viewport into a second scrollbar.
    `SpendSection` draws one `StatTimeChart` of input and output tokens and no
    by-model chart — the Models section is where a model is compared. Every
    figure — a key figure, a tile, a table column and a chart series —
    explains itself in one short sentence, shown on hover and on keyboard
    focus and wired as its accessible description (023).

- Goal details float over the board without a modal dialog, and a click on
  the scrim closes them
  (`ui/src/components/detail-panels.test.tsx::floats a goal over the board, and a click on its scrim closes the goal`,
  `ui/src/features/goals/goal-panel.test.tsx::renders a goal without a modal dialog`).
- Another `?goal=` swaps the open goal for that goal in the one pane
  (`ui/src/components/detail-panels.test.tsx::swaps the open goal for another goal the URL names`).
- An open goal leaves `<main>` alone in the shell's row, at its full width
  (`ui/src/components/app-shell.test.tsx::keeps the screen at full width behind an open goal`).
- A click on the scrim closes the pane through the same close as the close
  button
  (`ui/src/components/panel-sheet.test.tsx::closes the pane from a click on its scrim, as from its close button`).
- The close button is the pane's first tab stop, and Tab from the last stop
  stays in the pane
  (`ui/src/components/panel-sheet.test.tsx::puts the close button first in the pane's tab order`,
  `::keeps Tab inside the pane while it is open`).
- A task opened from a goal replaces it, mounting only the task's panel, and
  closes outright on Escape rather than falling back to the goal — even from
  a URL that names both
  (`ui/src/features/tasks/task-panel.test.tsx::replaces the goal with the task it opens, mounting only the one panel`,
  `::closes the pane outright on Escape, rather than falling back to the goal`,
  `ui/src/components/detail-panels.test.tsx::unmounts the goal outright once a task replaces it, and focuses the task`).
- The task panel's breadcrumb opens its goal in the pane's place
  (`ui/src/features/tasks/task-panel.test.tsx::opens the goal from the task's breadcrumb, replacing the task in the pane`,
  `ui/src/components/detail-panels.test.tsx::gives the task panel's breadcrumb the app's own focus ring, once the task names its goal`).
- The goal, the task and the session panel each open on the shared header's
  rows in order, with a title that truncates rather than wraps and carries
  the full text as its `title` attribute
  (`ui/src/features/goals/goal-panel.test.tsx::opens on the shared header: a truncating title, then status, id and stamps`,
  `ui/src/features/tasks/task-panel.test.tsx::opens on the shared header: a breadcrumb, a truncating title, then status, id and stamps`,
  `ui/src/features/sessions/session-panel.test.tsx::opens on the shared header: a truncating title, then status, id and stamps`).
- A session drilled into from a goal's or a task's own panel shows a
  breadcrumb back to it, and no "Back to …" button
  (`ui/src/features/goals/goal-panel.test.tsx::drills into a session with a breadcrumb back to the goal, and no Back button`,
  `ui/src/features/tasks/task-panel.test.tsx::drills into a session with a breadcrumb back to the task, and no Back button`).
- The session view renders no `h1` of its own, the shared header above it
  carrying the heading instead
  (`ui/src/features/sessions/session-detail-view.test.tsx::renders no h1 of its own, since the shared header above it carries the heading`).
- A goal panel with cached data and a failed refetch keeps showing that goal,
  under one title, with one inline notice above it
  (`ui/src/features/goals/goal-panel.test.tsx::keeps the cached goal on a failed refetch, with one title and one notice`).
- The goal panel's and the task panel's loading and error states render
  inside the pane's scrolling body
  (`ui/src/features/goals/goal-panel.test.tsx::renders the loading state inside the pane's scrolling body`,
  `::renders the goal's load failure inside the pane's scrolling body`,
  `ui/src/features/tasks/task-panel.test.tsx::renders the loading state inside the pane's scrolling body`,
  `::renders the task's load failure inside the pane's scrolling body`).
- Closing a task opened straight from the board returns focus to the card
  that opened it, same as any other panel
  (`ui/src/features/tasks/task-panel.test.tsx::closes to an empty pane and returns focus to the board card that opened it`).
- A pointer drag clamps and persists the pane width across a remount, and
  writes settings once, on release
  (`ui/src/components/panel-sheet.test.tsx::clamps a dragged pane width and restores it from settings on remount`,
  `::writes a dragged width to settings once, on release`).
- A pane closed in mid-drag still saves the width dragged to
  (`ui/src/components/panel-sheet.test.tsx::keeps the dragged width when the pane closes in mid-drag`).
- A double-click of the handle resets the pane to 36rem
  (`ui/src/components/panel-sheet.test.tsx::resets the pane to 36rem on a double-click of the handle`).
- The session frame supplies the remaining height through its scrolling body
  (`ui/src/features/sessions/session-panel.test.tsx::gives the console view the remaining pane height`).
- A console modal handles Escape without closing the surrounding pane
  (`ui/src/features/sessions/session-panel.test.tsx::keeps modal Escape separate from the pane close`).
- The Stats screen renders its five sections in order, and asks every family
  with the filters in its URL under the key `qk` names
  (`ui/src/routes/stats.test.tsx::renders the five sections in order, under
  one heading style`, `::asks every family with the filters in its URL,
  under the key qk names`); it leads with the key figures and lays the
  sections out as a card grid (`::leads with the key figures, read off the
  work, spend and models answers`, `::lays Models across the full width, and
  the other four out as a two-column card grid`); a
  task or a session update invalidates the stats
  (`ui/src/events/dispatch.test.ts::refetches every stat when a task or a
  session moves, since either may be a fact`); Stats is the last entry of the
  sidebar (`ui/src/components/app-shell.test.tsx::ends the navigation with
  stats, right after repositories and permissions`).
- Every figure on the Stats screen explains itself on hover and on keyboard
  focus, as its accessible description (023)
  (`ui/src/components/stats/stat-explain.test.tsx`,
  `ui/src/routes/stats.test.tsx::explains every key figure, on hover`, and
  each section's own `::explains every tile and every chart series, on
  hover` / `::explains every column of every seat table, on hover`).
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
- The sidebar lists Repositories after Permissions and before Stats, and
  `#/repositories` mounts the repositories screen, while a screen the app
  dropped leads nowhere
  (`ui/src/components/app-shell.test.tsx::ends the navigation with stats, right after repositories and permissions`,
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
- The repository dialog is a path, a base branch, a description, a
  permission mode and a default landing — not the merge-strategy or
  landing-briefing fields that used to sit there
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::takes a
  path, a base branch, a description, a permission mode and a default
  landing`).
- The repository dialog sends the permission mode picked for a new
  repository, starts an edit from the stored one, and the repositories
  screen shows each one's
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::sends the permission mode picked for it`,
  `::starts from the stored permission mode, and sends a new one`,
  `ui/src/features/repositories/repositories-page.test.tsx::lists what the daemon holds, and says so where a description is missing`).
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
- A model pin cuts its middle without cutting its effort in a panel fact and
  in a table row alike, and focus opens its whole value in a tooltip; a pin
  still wraps whole where a call site asks for that instead
  (`ui/src/features/models/model-pin.test.tsx::wraps every character of a pin without a tooltip`,
  `::keeps an effort visible beside a middle-cut model and opens the whole pin on focus`,
  `::holds a fact's pin to one line, cut in the middle, with the whole pin in its tooltip`,
  `::leaves out an empty effort and its at sign`).
- The sessions list uses the one-line pin and keeps its effort beside the
  middle-cut model
  (`ui/src/features/sessions/sessions-list.test.tsx::keeps the effort beside a middle-cut model and gives the row a whole-pin hint`).
- A listed model offers its own efforts, and an unlisted one takes free text
  (`ui/src/features/models/pin-picker.test.tsx::offers the efforts of the pinned model, the agent's own first, and stores the pick`,
  `::takes free text for a model of a known agent the catalog does not list`,
  `::takes free text for a model nothing has discovered`).
- The agent activity feed shows the daemon's summary and opens and closes the
  raw payload under its row
  (`ui/src/features/sessions/session-activity.test.tsx`).
- A session panel shows its reported context window with compact token
  figures beside a meter of how much is used, and hides an unreported one
  (`ui/src/features/sessions/session-detail-view.test.tsx::shows the reported context window with compact token figures`,
  `::shows the context window behind a 4rem meter sized to how much of it is used`,
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
- An agent's skills wrap onto as many lines as they need, clipping none of
  them, with the model it runs on below them in muted text and no middot
  before it; each skill still links to its skill and still shows its summary
  on hover
  (`ui/src/features/tasks/task-panel.test.tsx::shows the author's pin as it was staffed`,
  `::shows each reviewer slot's own pin, in review order`,
  `::wraps three skills, clipping none, even one with no hyphen to break on`,
  `ui/src/features/sessions/session-detail-view.test.tsx::shows the model the session was launched with, once`).
- A several-author task draws one block per author, with its skills, its
  model, its own branch and its own pick status each on its own line, and a
  clear gap between one author's block and the next
  (`ui/src/features/tasks/task-panel.test.tsx::shows every author's own branch, marking only the one the reviewers picked`).
- The Permissions screen's card renders the enabled switch, the model
  pickers, the two thresholds, Refresh and the status facts, and nothing about
  checkpoints or prompts
  (`ui/src/features/permissions/permissions-page.test.tsx::renders the
  switch, model pickers, thresholds, Refresh and facts without a refresh
  time`).
- The Permissions screen's card shows every fact of the settings row in the
  card, with no Details button, and the alert holds the last error once there
  is one
  (`ui/src/features/permissions/permissions-page.test.tsx::shows every fact
  the daemon answered with, with no Details button`,
  `::shows the last error as an alert at the top of the card`).
- The card's header is one row with the title, the state badge, the enabled
  switch and both icon buttons, and carries no description line any more
  (`ui/src/features/permissions/permissions-page.test.tsx::puts the title,
  the badge, the switch and both icon buttons in one header row`, `::drops
  the old description line under the title`).
- "Test a request" and "Refresh" render icon-only, each naming itself through
  a tooltip and an `aria-label` rather than visible text
  (`ui/src/features/permissions/ai-card.test.tsx::renders the header buttons
  icon-only, with a tooltip naming each`).
- The Flavour and Device selects share one inline row
  (`ui/src/features/permissions/permissions-page.test.tsx::the model pickers
  > puts Flavour and Device in one inline row`).
- The Status and hardware facts name what runs, the memory, the GPU and the
  machine, and every other fact is in the card with no "Details" popover
  (`ui/src/features/permissions/permissions-page.test.tsx::shows what runs,
  the memory and the GPU as facts`,
  `::shows no GPU where the hardware probe found none`,
  `ui/src/features/permissions/ai-card.test.tsx::shows the status and
  hardware facts in the card, with no Details popover`).
- Each Flavour option names what the flavour is and the devices it runs on,
  or why it runs on none, and each Device option names what the device is
  and notes a slow or unavailable one
  (`ui/src/features/permissions/permissions-page.test.tsx::the model pickers
  > disables an unsupported flavour and shows its reason`,
  `::the model pickers > disables an unavailable device and notes a slow
  one`).
- The switch sends `enabled` alone, and is disabled with the Python version
  found or that none was, where the daemon has none new enough
  (`ui/src/features/permissions/permissions-page.test.tsx::the enabled
  switch > sends enabled: true the moment it is turned on`,
  `::the enabled switch > is disabled with the version found, where Python is
  too old`,
  `::the enabled switch > is disabled and says not found, where no
  interpreter was found at all`).
- The allow and deny thresholds render as one range control: a two-handle
  slider over the three coloured zones with labels Allow, Ask and Deny in a
  row above the track, positioned at each zone's center and hidden where its
  own zone is too narrow, and both number inputs in one row below it — a green
  dot for the allow input, a red dot for the deny input — each named for the
  threshold it holds and showing the row's own value to four fixed decimals,
  settled on mount, after a drag and after a commit, and left alone while
  typed into. The control has no tick row and no description, so it is
  visibly shorter than before
  (`ui/src/features/permissions/threshold-range.test.tsx::shows the track's
  three zones and the handles named for what they hold`,
  `::shows the two number inputs, named and valued to four decimals for the
  row they hold`,
  `::shows a zone-coloured dot beside each input's label`,
  `::puts both inputs in one row below the track, not under the handles`,
  `::keeps the danger readout inside the card at either end of the track`,
  `::formats an input to four decimals on first render, after a drag, and
  after a commit, but not while typing`,
  `::positions zone labels at the center of each zone and updates them when
  thresholds move`,
  `::hides only the narrow zone label`).
- A handle released at a new value, or a number field left or Entered, sends
  only the field that changed, clamped to 0–1, up to four decimals; neither
  handle can reach or pass the other; each thumb has `aria-valuetext` with the
  value to four decimals (for example "Allow threshold 0.3000")
  (`ui/src/features/permissions/threshold-range.test.tsx::a handle released
  at a new value > sends one PUT with only the allow field`,
  `::a handle released at a new value > sends one PUT with only the deny
  field`,
  `::a handle released at a new value > does not let the allow handle reach
  or pass the deny handle`,
  `::a handle released at a new value > does not let the deny handle reach
  or pass the allow handle`,
  `::an input left or Entered > sends one PUT with only the allow field, four
  decimals kept`,
  `::an input left or Entered > sends one PUT with only the deny field, on
  Enter`,
  `::clamps a typed allow value of 5 to 1 before sending`,
  `::clamps a typed deny value of -1 to 0 before sending`,
  `::gives each thumb an aria-valuetext with the value to four decimals`,
  `ui/src/features/permissions/permissions-page.test.tsx::sends the allow
  threshold typed, once the field is left`,
  `::sends the deny threshold typed, once the field is left`).
- The range control takes an optional danger marker on its track, taller than
  the track so it reads over the zone colours, drawn with its own accessible
  label formatted to four decimals and that same value written beside it
  (`ui/src/features/permissions/threshold-range.test.tsx::draws no danger
  marker where none is set, and one labelled with its value to four decimals
  where it is`).
- Refresh posts once, and is disabled while the model is off or already
  installing
  (`ui/src/features/permissions/permissions-page.test.tsx::Refresh > posts to
  the refresh endpoint`,
  `::Refresh > is disabled while the model is off`,
  `::Refresh > is disabled while an install is already running`).
- A refusal of any of the above is toasted with the daemon's own message, and
  a threshold's refusal puts the value back to the row the daemon still holds
  (`ui/src/features/permissions/permissions-page.test.tsx::toasts the
  daemon's own message on a refusal`,
  `::toasts the daemon's own message on a refused threshold, and puts the
  value back`,
  `ui/src/features/permissions/threshold-range.test.tsx::toasts the daemon's
  own message on a refusal, and puts the value back`).
- `ai_permissions_updated` replaces the cached settings row whole, so a card
  that read `installing` reads `ready`
  (`ui/src/events/dispatch.test.ts::ai permissions events > replaces the
  cached status whole, so a card that read installing reads ready`).
- The repository dialog offers `AI` among the permission modes and sends it
  as `ai`, and an `ai_disabled` refusal lands on that field naming the
  Permissions screen
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::offers AI
  among the permission modes, and sends it as ai`,
  `::puts an ai_disabled refusal on the permission mode field, pointing at
  the Permissions screen`).
- The repository dialog sends the default landing picked for a new
  repository, and starts an edit from the stored one
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::sends the
  default landing picked for it`,
  `::starts from the stored default landing, and sends a new one`).
- The goal dialog's landing starts at the first repository's own default
  once one is picked, stands once chosen by hand even if a different
  repository becomes first, and is sent whichever way it was settled
  (`ui/src/features/goals/create-goal-dialog.test.tsx::starts out at merge,
  before any repository is picked`,
  `::preselects the first picked repository's own default landing`,
  `::keeps the merge default where the first picked repository uses it`,
  `::sends the preselected landing on submit`,
  `::sends a landing picked by hand instead of the repository's default`,
  `::keeps a hand-picked landing once a different repository becomes the
  first`).
- The goal panel's facts show the goal's landing, and name each repository by
  its folder with its base branch bracketed after it and a feature-branch
  goal's own branch after that once the plan has cut one — the full path
  stays in a tooltip and stays copyable
  (`ui/src/features/goals/goal-panel.test.tsx::shows the goal's landing
  among its facts`,
  `::names each repository by its folder, with its base branch bracketed and
  its goal branch after`,
  `::holds the full path of each repository in its tooltip, and keeps it
  copyable`).
- A screen's name appears once, as the header's only `h1`, with that screen's
  own actions at the header's end, and the shell renders no footer of its
  own — the sidebar's last child is the daemon connection status, whose click
  still opens the logs drawer
  (`ui/src/components/app-shell.test.tsx::shows the screen's name once, as
  the header's only h1, with its actions at the header's end`,
  `::renders no footer, and ends the sidebar with the connection status,
  whose click opens the logs drawer`).
- A `PageHeader` carries no heading of its own: its `actions` portal into the
  header through `PageHeaderContext`, and its `description` becomes the
  tooltip of the header's title; mounted on its own, with no shell around it,
  it renders `actions` in place instead
  (`ui/src/components/page-header.test.tsx::shows its description as the
  tooltip of the shell's header title`,
  `::renders its actions in place, with no heading, when there is no shell to
  hand them to`).
- The rail drops the wordmark for the mark alone, and the connection status
  for the dot alone, each named by its own tooltip
  (`ui/src/components/app-shell.test.tsx::replaces the wordmark with the
  mark alone in the rail, next to the status dot`,
  `ui/src/components/connection-status.test.tsx::shows the dot alone in the
  rail, named by a tooltip instead of a label`).

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
