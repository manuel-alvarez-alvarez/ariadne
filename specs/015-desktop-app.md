---
id: desktop-app
status: current
updated: 2026-10-10
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/features/**/*.test.ts
  - ui/src/routes/**/*.test.tsx
  - ui/src/api/**/*.test.ts
  - ui/src/components/**/*.test.tsx
  - ui/src/lib/**/*.test.ts
  - ui/src/lib/tauri-config.test.ts
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
   screen's own actions at the header's end. Goal, task, session, and pull request (026) details
   occupy one pane that floats over the screen at its right edge. The URL
   selects its contents. The screen keeps its full width and layout behind
   the pane, under a scrim that uses the dialog's own `SCRIM` token. A click
   on the scrim closes the pane, the same as the close button, and Tab stays
   inside the pane while it is open. The pane slides in from the right and
   the scrim fades in, unless the system asks for reduced motion. The close
   button sits in the pane's header, as its first tab stop. A toggle beside
   it expands the pane to near-fullscreen, centered, the same size as the
   console's own expanded modal; a second press collapses it back to its
   docked width, and the resize handle hides while it is expanded. Each open
   starts docked — nothing remembers the expanded choice. The left handle
   resizes the docked pane by pointer or keyboard between 24rem and 60% of
   the window. A drag saves the width once, on release, and a double-click
   resets it to the 36rem default; settings preserve the choice.
   Below `md`, the pane covers the screen at full width. The pane holds one
   panel at a time: a task opened from a goal replaces the goal's panel
   rather than stacking on it, carrying a breadcrumb back that reopens the
   goal in its place. Session drill-downs use that pane too. Escape inside
   it, or its own close control, closes the pane outright — never back to a
   goal a task replaced, even from a URL that names both. Closing preserves
   the existing history and, where an opener is still on the screen, focus
   return. Replacing a panel keeps focus inside the new pane. An old panel
   gives the first opener to its replacement. Closing the final pane returns
   focus to that opener if it remains mounted.
   If a focused control disappears, focus returns to the pane itself unless
   another control already has focus. This includes controls inside the pane's
   modal portals: cancelling a goal or task, retrying a task, and collapsing
   the expanded console or diff. Open dialogs retain their own focus.
   The header does not shrink, and the body scrolls within the
   remaining height, with one full-height child for the session view.
3. Screens: the goals board (swimlanes plus an attention strip) has Active,
   All and Finished status segments. A status-menu icon holds a custom
   selection. Each lane header shows a progress bar, done/total and tokens.
   Its title hint carries the created stamp. The bar and a collapsed lane
   retain the lane summary. The task
   panel (facts, messages listed oldest first with both agents named by skills,
   diff, history), sessions — Ariadne's own and every
   outside conversation an ACP agent stored on its own, merged into one
   listing, each shown in its console — skills, repositories, the agents of
   the daemon's ACP registry with their launch flags and the models each may
   be staffed on, Workflows — shipped and user workflows grouped separately,
   with a document editor and a parsed preview drawn as a numbered pipeline
   — Permissions — the AI permission model's settings behind the `ai`
   permission mode (022) — Stats
   (rule 37), and a daemon-logs drawer. The Skills and Workflows screens each
   put a filterable combobox at the top rather than a left-hand list: its
   trigger reads as the selected row or a placeholder, its popup groups
   "Shipped with Ariadne" apart from "Yours", a row shows its name and (a
   workflow's step titles joined by " · ", or a skill's summary line) under
   it, and typing narrows by either. Picking a row, with the keyboard or the
   pointer alike, puts its name in the URL and opens its editor below the
   combobox, full width; the unsaved-changes guard runs on a pick exactly as
   it runs on any other navigation away from a dirty draft. Neither screen
   shows a list panel at any width. `workflows-page.test.tsx`,
   `workflow-editor.test.tsx`, and `workflow-preview.test.tsx` prove the
   workflow screen rules. The editor and preview share the screen in equal
   columns, and a parser refusal marks its source line in the editor
   (`workflow-editor.test.tsx::puts the editor and preview in equal
   columns`, `workflow-editor.test.tsx::parses each draft once for the
   editor and preview`, `workflow-editor.test.tsx::marks the line the parser
   refuses in the editor`, and `workflow-language.test.ts::classifies
   workflow names, columns, metadata, and descriptions`). The editor also
   completes and explains the document as it is typed: at the start of a
   column's body line it offers
   `skills:`, `rank:` and `gate:`; after `rank:` it offers `fast`, `balanced`,
   `frontier` and `local`; after `gate:` it offers `committed`, `pushed`,
   `merged` and `request-merged`; and after `skills:`, or after a comma in
   its list, it offers the skill catalog's names with each one's summary as
   the detail, leaving out `orchestration`, `pr-reviewer` and a skill named
   anywhere else on the same line, on either side of the cursor. Hovering a
   key, or a rank or gate value, shows one sentence on what it means, right
   up to the character before the value starts even with no space after the
   colon; hovering a skill name shows its summary, or that the catalog has
   none by that name
   (`workflow-help.test.ts::offers the body keys at the start of a body
   line`, `::offers the rank words after rank:`, `::offers the gate words
   after gate:`, `::offers skill names after skills:, with each summary as
   the detail`, `::never offers orchestration or pr-reviewer`, `::offers
   skill names after a comma, excluding the one the line already names`,
   `::excludes a skill already named later on the line, inserting before
   it`, `::excludes skills named on both sides, inserting between them`,
   `::explains the skills key`, `::explains the rank key`, `::explains the
   gate key`, `::explains the rank value frontier`, `::explains the rank
   value balanced`, `::explains the rank value fast`, `::explains the rank
   value local`, `::explains the gate value committed`, `::explains the
   gate value pushed`, `::explains the gate value merged`, `::explains the
   gate value request-merged`, `::explains a rank value directly after the
   colon, with no space`, `::explains a gate value directly after the
   colon, with no space`, `::explains a skill name directly after the
   colon, with no space`, `::shows a known skill's summary`, `::says when
   the catalog has no such skill`). The preview draws
   each column as a numbered step on a vertical rail, a card holding its
   title, id, description, skills and rank in that order, and ends the rail
   with a marker for the end of the task; a step's gate shows as a labelled
   chip on the connector below it, and a step with no gate leaves that
   connector bare (`workflow-preview.test.tsx::draws the steps as a numbered
   pipeline, in order, ending with a marker`,
   `::shows a card's title, id, description, skills and rank, in that
   order`, `::shows a step's gate as a chip on its connector, and none
   where a step has no gate`, and `::shows a parser refusal at its line,
   and dims the last good preview`). `skills-page.test.tsx` proves the
   combobox rules the skills screen shares with it. The New workflow dialog
   gives its document the same editor and a live preview beside it, two
   columns wide: a parser refusal marks its line in the editor the same way,
   and the editor offers the same skill autocomplete and hover help. Its
   document opens on the template named after the name field and keeps
   following it there until the document itself is edited, after which the
   name no longer rewrites it; the dialog's own dirty-close guard answers to
   either field (`create-workflow-dialog.test.tsx::puts the editor and the
   preview in two columns`, `::sends the live document to the parser, and
   redraws the preview from its answer`, `::marks a parse error on its line
   in the editor, and shows it in the preview`, `::completes a skill name
   from the catalog after skills: in the dialog editor`, `::explains a
   skill named on a skills: line, through the dialog editor's hover help`,
   `::opens on the template named after the placeholder`, `::follows the
   name until the document is touched, so nobody types it twice`, `::stops
   following the name once the document itself is edited`, and `::asks
   before dropping a document edited by
   hand`).
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
10. On macOS, the window accepts the first mouse click, so a click on an
    inactive window also operates the control under the pointer.
11. The primary surface is the macOS Tauri window (WebKit): a layout change is
    verified there, not only in a browser.
12. The app is checked by `npm test`, `npm run typecheck`, `npm run lint` and
    `npm run check:unused` before a commit.
13. A goal, and on a task of a goal a workflow runs each column's staffing
    row, each name a concrete `<agent>:<model>` before their form can submit,
    where `<agent>` is a registry agent id (011), and an empty effort stays
    valid and uses that model's default effort.
14. The model picker lists concrete catalog entries only, under one heading
    per registry agent, in the order the catalog first names each agent. No
    screen shows an automatic or default model; `auto` is an effort choice
    only.
15. The client checks only the shape of a pin: one `:`, with text on both
    sides of it. Which agents exist is the daemon's answer, so the client
    refuses no agent id by name. Only the first `:` splits the pin; a later
    one is part of the model.
16. A model the catalog lists takes only the efforts the catalog gives it. A
    model the catalog does not list takes a free-text effort, as the daemon
    takes any effort that is not blank for such a model (011).
17. The task form's skill boxes suggest only skills that can staff a task
    agent; the orchestrator's own playbook is not among them, for any
    column's agent. The skills screen marks that playbook beside its built-in
    mark, staying editable and resettable like any other shipped skill.
18. The agent activity feed shows each event by its kind in plain words: Tool
    call, Tool result, Permission asked, Permission answered, Agent said, and
    so on for every kind the daemon records; an unknown kind shows its raw name.
    A colored dot from the status ramp — warn for permission events, danger for
    errors, active for agent messages, pending for tool calls — precedes the
    plain words. The summary is the daemon's one-line gist, or derived from the
    payload when empty: the tool name and its first argument, showing `Read
    /tmp/x.png` or `Bash git status`. Consecutive rows of the same kind and the
    same tool fold into one, such as `Tool call · Read /file ×7`, which expands
    to the individual rows. Each row still expands to its raw payload.
19. The sessions screen lists both kinds of session in one table, newest
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
20. The goal, task and session panels, and the session drill-downs inside the
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
    screen; a stepped task's own Agents fact and step strip are rule 40's.
21. Every session is shown in its console, as the CLI draws it: a terminal
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
    remains console input in the panel and in the modal alike; Escape
    outside the console closes the panel or the modal. ⌘Escape leaves a
    focused console without sending it Escape or closing anything itself: it
    moves focus to the panel's Close button, or to the modal's Collapse
    button, so the plain Escape that follows closes what is now focused.
22. Every key press is sent as a `key` message, the DOM key mapped one to
    one onto crossterm's code and modifiers — a printable character as
    itself, the named keys by name, F1 to F12 by number, Shift+Tab as
    `back_tab` — and every paste as a `paste` with its text; xterm.js's own
    key handling is bypassed. Three things are left to the browser rather
    than sent: a bare modifier, a chord held with the command key on
    macOS, and Ctrl+Shift+C and Ctrl+Shift+V, so copying the selection and
    the paste event that becomes the `paste` keep working. Option on macOS
    reads the letter off the physical key, so Alt-B is a word left there
    too. Keys typed before the socket is open go nowhere.
23. The pane takes the app's colours and font from the tokens the rest of
    the UI is drawn in, in both themes, re-read when the theme switches:
    the six named terminal colours map onto the status ramp. It fills the
    box it is given, refits when that box changes, and the daemon redraws
    at the new size. It draws on WebGL where the webview has a context, so
    box-drawing glyphs — the input box's rules, a table's rule — are the
    emulator's own and join into one line; without one it keeps the DOM
    renderer, which takes them from the font.
24. A drop of the socket is retried on the event stream's backoff, and the
    pane says it is reconnecting meanwhile. A close the daemon meant — the
    session ended, Ctrl-C twice, Ctrl-D — ends the console instead: the
    pane says the console closed, or that the session ended when the
    daemon's last status frame said so, and a Reopen button opens another.
25. A session's view has two tabs over one space: the console, open by
    default, and the agent activity feed. The tab is in the URL (`?tab=`);
    the console's tab is `terminal` on the wire, so an older link still
    opens it. A tab value that is not one of the two opens the console.
    Leaving the console's tab closes its socket, and coming back opens a
    new one.
26. The sessions list shows the pin the session was launched on
    (`<agent>:<model>`): the row cuts the model id in the middle when needed,
    keeps the effort after an `@` readable, and its hint holds the whole pin
    (`ui/src/features/sessions/sessions-list.test.tsx::keeps the effort beside
    a middle-cut model and gives the row a whole-pin hint`).
27. A row of the attention strip for an agent blocked on a permission or an
    input prompt opens that session's console with `?focus=`, so the terminal
    takes the keyboard on arrival.
28. Above the console, a session blocked on a permission or an input prompt
    shows a banner that says where to answer: the picker in the console for
    a permission, the console's input for a question. A session that has
    ended while blocked is told to resume first. No other attention reason
    shows the banner.
29. The typed keyboard chords (`n`, `g` then a letter, `?`, `[`) are ignored
    while a field, an editor or a session's terminal has the keyboard. The
    held ⌘ chords (⌘K, ⌘,) are not: they fire from a field or a session's
    console too, the same as every browser's own ⌘K, but neither fires
    while a dialog or a menu is up.
30. The agents screen has one tab per registry agent from `GET /v1/agents`,
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
31. A session panel shows a reported context window as `<used> / <size>`,
    using the compact spelling of token figures, beside a 4rem meter of how
    much of it is used. It shows no context fact before the agent reports
    one, and it never shows a cost.
32. Every session outside a cancelled goal offers Switch beside its session
    actions. Its dialog starts on the session's pin, checks the pin's
    `<agent>:<model>` shape, and posts its model and optional effort to
    `POST /v1/sessions/{id}/switch`. A refusal shows the daemon's message; a
    successor id replaces the open session while a same id leaves it selected.
    A switched session links to the session it continues
    (`ui/src/features/sessions/session-actions.test.tsx`).
33. Beside each model's switch, a picker shows its `rank` from `GET
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
34. The Permissions screen's AI tab holds one card, "AI", with an alert at the
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
    daemon's own CLI-flavoured words.
35. The Permissions screen's Learned tab lists every learned approval
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
    `::shows the output's label, both probabilities, danger, both thresholds,
    the cap and the tags where the model was called`,
    `ui/src/events/dispatch.test.ts::patches created and updated details and
    refetches lists`).
36. The AI tab's card carries a "Test a request" button in its header button
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
37. The Stats screen is at `#/stats`, titled `Stats`, last in the sidebar
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
38. Toasts show at the bottom left, clear of the header and the pane
    (`ui/src/components/ui/sonner.test.tsx::renders toasts at the bottom left`).
39. A goal a workflow runs (030) is shown by its columns. Its lane on the
    goals board draws its own column row: Pending (pending, ready, and failed
    outlined in danger), one column per step in order, titled by the step,
    then Done (finished); cancelled stays off the board. A card sits in the
    column of its task's `step`. A lane of a goal with no steps draws Pending,
    In progress and Done, and a task of that goal under way sits in In
    progress. Each lane carries its own column row, and its grid takes
    as many columns as it has, at the same width floors; the board scrolls
    sideways to its widest lane. A card whose status shows names the step
    where the status said `in progress`.
40. The task panel of a stepped task draws a step strip under its facts: one
    segment per column, the current one highlighted, the ones behind it
    marked done, each segment's hint the column's description. Its Agents
    fact lists one row per column — the column, the skills, the pin and a
    link to its live session — and no author or reviewer fact. The history
    names a step move by its `from_step` and `to_step` columns.
41. The task form shows one row per column of the goal's workflow, prefilled
    with the column's skills, with the column's preferred rank beside the pin
    picker, and sends `agents`, seat `agent` and the column's `step` each, on
    create and on edit, where an edit replaces the whole list; a goal with no
    steps gets no staffing row at all.
42. The goal dialog has a workflow picker over the catalog, defaulted to the
    first picked repository's `default_workflow`, and sends `workflow` in
    place of a landing, or nothing where none is picked; it has no landing
    select. The repository dialog requires a default workflow — refusing to
    register or save without one picked — and has no default landing select.
43. A session of seat `agent` is named by the column its agent staffs: its
    badge on the sessions screen, the attention list and the palette. The
    sessions screen's role filter offers the seat.
44. The task panel's sessions table names each row by the title of its
    agent's workflow step, found through the task's own `agents` and the
    goal's own `steps` rather than a daemon call: the staffed agent of
    `task_agent_id`, its `step`, and that step's `title` — the id itself
    where the workflow no longer names one. A row with no staffed agent
    shows no step. The goal panel's sessions table and the Sessions screen
    are unchanged.
45. The task panel's History tab renders each transition's reason as
    Markdown, through the same renderer as descriptions and messages, so a
    reason an agent wrote with lists, code or links reads as it does
    everywhere else.
46. A click on an external `<a>`, anywhere in the app, opens it in the
    system's default browser instead of the webview, which otherwise ignores
    `target="_blank"` and has no opener wired in. One document-level handler
    at the app root, over `tauri-plugin-opener`, decides this by the anchor's
    resolved origin rather than its `target`; an in-app link, whose href
    resolves onto the app's own origin, is left alone. Outside Tauri — the
    Vite dev server, and every test — the handler does nothing and a plain
    browser's own link behavior applies.

## Acceptance criteria

- Goal details float over the board without a modal dialog, a click on the
  scrim closes them, and another `?goal=` swaps the open goal for that goal
  in the one pane
  (`ui/src/components/detail-panels.test.tsx::floats a goal over the board, and a click on its scrim closes the goal`,
  `::swaps the open goal for another goal the URL names`).
- An open goal leaves `<main>` alone in the shell's row, at its full width
  (`ui/src/components/app-shell.test.tsx::keeps the screen at full width behind an open goal`).
- A click on the scrim closes the pane through the same close as the close
  button
  (`ui/src/components/panel-sheet.test.tsx::closes the pane from a click on its scrim, as from its close button`).
- The close button is the pane's first tab stop, and Tab from the last stop
  stays in the pane
  (`ui/src/components/panel-sheet.test.tsx::puts the close button first in the pane's tab order`,
  `::keeps Tab inside the pane while it is open`).
- A task opened from a goal replaces the goal's own session view entirely,
  mounting only the task's panel, and Escape empties the pane rather than
  falling back to the goal it replaced
  (`ui/src/components/detail-panels.test.tsx::replaces a goal's own session view with the task it names, and Escape empties the pane rather than returning to it`,
  `::unmounts the goal outright once a task replaces it, and focuses the task`).
- The task panel's breadcrumb to its goal wears the app's own focus ring and
  is the pane's first focused control once the task names its goal
  (`ui/src/components/detail-panels.test.tsx::gives the task panel's breadcrumb the app's own focus ring, once the task names its goal`).
- Closing a panel returns focus to the board link or card that opened it
  (`ui/src/components/panel-sheet.test.tsx::leaves Escape on the board alone and returns focus when the pane closes`).
- Opening a goal from the board and then its task keeps focus inside the new pane
  (`ui/src/components/pane-focus.test.tsx::keeps focus in the task pane after opening a goal from the board`).
- Closing a replacement task or returning through its goal breadcrumb preserves the first board opener
  (`ui/src/components/pane-focus.test.tsx::returns focus to the board opener after closing the replacement task`,
  `::returns focus to the board opener after closing the replacement goal breadcrumb`).
- Removing an action or closing an expanded view restores focus inside the pane
  (`ui/src/components/pane-focus.test.tsx::keeps focus in the pane after confirming Cancel goal`,
  `::keeps focus in the pane after confirming Cancel task`,
  `::keeps focus in the pane after Retry task removes its button`,
  `::keeps focus in the pane after collapsing the console`,
  `::keeps focus in the pane after collapsing the diff`).
- A pointer drag clamps and persists the pane width across a remount, and
  writes settings once, on release
  (`ui/src/components/panel-sheet.test.tsx::clamps a dragged pane width and restores it from settings on remount`,
  `::writes a dragged width to settings once, on release`).
- A pane closed in mid-drag still saves the width dragged to
  (`ui/src/components/panel-sheet.test.tsx::keeps the dragged width when the pane closes in mid-drag`).
- A double-click of the handle resets the pane to 36rem
  (`ui/src/components/panel-sheet.test.tsx::resets the pane to 36rem on a double-click of the handle`).
- The pane's scrim carries the dialog's own `SCRIM` token, not a scrim of its
  own
  (`ui/src/components/panel-sheet.test.tsx::darkens the screen with the dialog's own SCRIM token`).
- The toggle expands the docked pane to near-fullscreen, hides the resize
  handle, and a second press collapses it back to the stored docked width
  (`ui/src/components/panel-sheet.test.tsx::expands the pane to near-fullscreen and back on a second press`).
- The scrim click and Escape still close an expanded pane
  (`ui/src/components/panel-sheet.test.tsx::still closes the expanded pane from its scrim and from Escape`).
- Tab stays inside an expanded pane, over the close and toggle buttons, with
  no resize handle to land on
  (`ui/src/components/panel-sheet.test.tsx::keeps Tab inside the expanded pane, past the close and toggle, with no resize handle to land on`).
- Closing an expanded pane returns focus to the board opener, the same as a
  docked one
  (`ui/src/components/pane-focus.test.tsx::returns focus to the board opener after closing an expanded pane`).
- The Stats screen renders its five sections in order, and asks every family
  with the filters in its URL under the key `qk` names
  (`ui/src/routes/stats.test.tsx::renders the five sections in order, under
  one heading style`, `::asks every family with the filters in its URL,
  under the key qk names`); it leads with the key figures and lays the
  sections out as a card grid (`::leads with the key figures, read off the
  work, spend and attention answers`, `::lays Models across the full width, and
  the other four out as a two-column card grid`); a
  task, a session or a goal update invalidates the stats
  (`ui/src/events/dispatch.test.ts::refetches every stat when a task, a
  session or a goal moves, since any may be a fact`); Stats is the last entry of the
  sidebar (`ui/src/components/app-shell.test.tsx::ends the navigation with
  stats, and lists Forge beside the other screens`).
- Every key figure on the Stats screen, and every tile and chart series of its
  own Work, Time, Spend and Attention sections, explains itself on hover and
  on keyboard focus, as its accessible description (023)
  (`ui/src/components/stats/stat-explain.test.tsx`,
  `ui/src/routes/stats.test.tsx::explains every key figure, on hover`,
  `ui/src/components/stats/work-section.test.tsx::explains every tile and
  every chart series, on hover`, and the same title in
  `time-section.test.tsx`, `spend-section.test.tsx` and
  `attention-section.test.tsx`).
- 95 test files cover the features, the API layer and the event stream;
  most screens are asserted in their own `*.test.tsx` beside them, though a
  few (noted under Known gap) are exercised only through the panels and
  boards that mount them.
- The sidebar lists Repositories after Permissions and before Stats, and
  `#/repositories` mounts the repositories screen, while a screen the app
  dropped leads nowhere
  (`ui/src/components/app-shell.test.tsx::ends the navigation with stats, and lists Forge beside the other screens`,
  `ui/src/routes/router.test.tsx::mounts the repositories screen at #/repositories`,
  `ui/src/routes/router.test.tsx::leads nowhere from a screen the app no longer has`).
- The palette opens `#/repositories` on a picked repository
  (`ui/src/features/command-palette/command-palette.test.tsx::opens the repositories screen on a repository from the palette`).
- The task's Messages tab reads one oldest-first list, names both agents by
  their skills, shows its empty state, and refetches after `message_sent`
  (`ui/src/features/tasks/task-messages.test.tsx`).
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
- The skills screen's combobox groups the shipped skills apart from the
  user's own, and marks a shipped skill somebody has rewritten with an
  "edited" badge nothing else earns
  (`ui/src/features/skills/skills-page.test.tsx::groups the shipped skills
  apart from the ones you wrote`, `::badges only a shipped skill somebody has
  rewritten`), and offers reset for the first and delete for the second and
  never the other way round (`ui/src/features/skills/skill-editor.test.tsx`)
  — the rule of 017 read from the client side.
- The orchestrator's own playbook is marked beside the built-in mark
  (`ui/src/features/skills/skill-editor.test.tsx::marks the orchestrator's own
  playbook as not a task staffing choice`).
- The skill editor's draft is never overwritten out from under a typing user:
  it follows the selected skill's document only while the draft still reads
  exactly what it was last set from, and holds its ground once it does not —
  through a change from elsewhere (the CLI, Reset, another window) and
  through the refetch of a save of its own alike
  (`ui/src/features/skills/skill-editor.test.tsx::follows the row when the
  document changes under it`, `::keeps a dirty draft when the row changes
  underneath it, and says so`, `::keeps text typed after a click on Save,
  once the saved document comes back`). A draft held back this way says a
  newer version is waiting, with a way to load it in the draft's place
  (`ui/src/features/skills/skill-editor.test.tsx::loads the new version on
  request, replacing the draft`).
- The skills screen asks before a dirty draft is left — by picking another
  skill from the combobox, by Back, or by a route to another screen entirely,
  such as a sidebar link — and only then: Keep editing cancels the move and
  keeps the draft, Discard carries it out, and a clean draft never asks at all
  (`ui/src/features/skills/skills-page.test.tsx::asks before switching to a
  different skill from the combobox`, `::keeps the draft and stays, on Keep
  editing`, `::discards the draft and switches, on Discard`, `::asks before
  Back leaves a dirty skill, too`, `::asks before a route to another screen
  leaves a dirty skill, too`, `::leaves a clean skill with no prompt`).
- The repository dialog requires a default workflow, refusing to register
  without one picked, and has no default landing, merge-strategy or
  landing-briefing field
  (`ui/src/features/repositories/repository-form-dialog.test.tsx::refuses to register without a workflow picked`).
- The repositories screen lists what the daemon holds and says so where a
  description is missing, and shows each repository's default workflow,
  opening that workflow when its name is selected
  (`ui/src/features/repositories/repositories-page.test.tsx::lists what the daemon holds, and says so where a description is missing`,
  `::shows each repository workflow and opens its workflow`).
- An attention toast opens the blocked session and finishes dismissal before
  the test removes its browser environment
  (`ui/src/features/goals/attention-alerts.test.tsx::raises one toast for an agent that gets stuck on another screen`).
- Unused declared dependencies, exports only tests import, and orphan source files fail `npm run check:unused`.
- The goal dialog offers a workflow picker and no landing select
  (`ui/src/features/goals/create-goal-dialog.test.tsx::offers a workflow and no landing`),
  and the task form renders one staffing row per workflow column, with none
  for a goal with no workflow
  (`ui/src/features/tasks/task-form-dialog.test.tsx::renders one staffing row for each workflow column`).
- A model reference is refused unless it has text on both sides of exactly
  one colon, which is what the goal dialog's and the task form's own model
  fields both reuse
  (`ui/src/features/models/model-ref.test.ts::refuses an empty model`,
  `::refuses one half on its own by showing where the other goes`,
  `::refuses a leading colon, which names no agent`,
  `::refuses a trailing colon, which names no model`).
- The picker lists only concrete model ids, grouped by agent
  (`ui/src/features/models/pin-picker.test.tsx::offers only concrete catalog models, grouped by agent`).
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
- `qk.outsideSessions` refetches on `session_created`, since a resume may
  move one of its rows into the Ariadne half, and leaves it alone on
  `session_updated`
  (`ui/src/events/dispatch.test.ts::refetches the outside lists when a session is created, since it may hold one of their rows`,
  `::leaves the outside lists alone when a session only moves on`).
- The outside-sessions page, its route and the adoption dialog are gone
  (`ui/src/routes/router.test.tsx::leads nowhere from the outside-sessions screen the merge dropped`).
- The terminal pane sends its size before anything else, and nothing typed
  before the socket is open
  (`ui/src/features/sessions/session-terminal.test.tsx::sends its size before anything else`).
- The terminal opens in a near-fullscreen modal, closes its previous socket
  before its modal socket opens and resizes, restores a fresh panel socket on
  collapse, and gives a focused console Escape in the panel and in the modal
  alike, while Escape outside the console still closes the pane or dismisses
  the modal
  (`ui/src/features/sessions/session-terminal.test.tsx::expands the console into a near-fullscreen modal`,
  `::closes the panel socket before opening and resizing the modal console`,
  `::collapses the modal console into the panel on a fresh socket`,
  `::keeps the modal open when focused Escape belongs to the console`,
  `::closes the modal when Escape occurs outside the console`,
  `ui/src/components/panel-sheet.test.tsx::leaves Escape on the board alone and returns focus when the pane closes`).
- ⌘Escape leaves a focused console for the Collapse button in the modal,
  sending nothing to the agent and closing neither by itself
  (`ui/src/features/sessions/session-terminal.test.tsx::moves focus to Collapse on ⌘Escape, and sends nothing to the console`).
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
- ⌘K opens the command palette from a text field and from a session's
  console alike, and does nothing while a dialog is up
  (`ui/src/hooks/use-global-shortcuts.test.tsx::opens the palette on ⌘K from a text field`,
  `::opens the palette on ⌘K from a session's console`,
  `::does nothing on ⌘K while a dialog is up`).
- The cheat sheet lists ⌘Escape
  (`ui/src/components/keyboard-shortcuts-dialog.test.tsx::lists every chord, screen by screen`).
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
- A lane of a stepped goal draws Pending, its columns and Done, and puts each
  card in its step's column; a lane without steps draws Pending, In progress
  and Done, with a task under way in the middle one
  (`ui/src/features/goals/goal-swimlanes.test.tsx::draws each goal workflow columns`,
  `::draws an in-progress task of a stepless goal in its own column`).
- The task panel of a stepped task lists one agent per column
  (`ui/src/features/tasks/task-panel.test.tsx::shows workflow agents by column`).
- The task form maps one agent to every workflow column, prefilled with the
  column's skills, and sends the complete staffing on create and on update
  (`ui/src/features/tasks/task-form-values.test.ts::maps one workflow agent for every column`,
  `::sends the complete workflow staffing on create and update`,
  `ui/src/features/tasks/task-form-dialog.test.tsx::renders one staffing row for each workflow column`).
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
- On macOS, a click on an inactive window operates the control under the pointer
  (`ui/src/lib/tauri-config.test.ts::lets the first click operate a control on an inactive window`).
- The rail drops the wordmark for the mark alone, and the connection status
  for the dot alone, each named by its own tooltip
  (`ui/src/components/app-shell.test.tsx::replaces the wordmark with the
  mark alone in the rail, next to the status dot`,
  `ui/src/components/connection-status.test.tsx::shows the dot alone in the
  rail, named by a tooltip instead of a label`).
- The task panel's sessions table names a row by its agent's workflow step
  title, falls back to the step id where the workflow no longer names one,
  and names nothing for a row with no staffed agent
  (`ui/src/features/tasks/task-sessions.test.tsx::names a session row by the
  title of its agent's workflow step`, `::names no step for a session with
  no staffed agent`, `::falls back to the step id where the workflow no
  longer names its title`,
  `ui/src/features/sessions/sessions-list.test.tsx::names a row by its
  workflow step where the caller gives one, and names none where it gives
  none`).
- The History tab renders a transition's reason as Markdown: lists, inline
  code and links, a link opening outside the app
  (`ui/src/features/tasks/task-history.test.tsx::renders a transition's
  reason as Markdown`).
- The app-root handler opens an external link through the opener plugin and
  leaves an in-app link alone, and does neither outside Tauri
  (`ui/src/hooks/use-open-external-links.test.tsx::opens an external link
  through the opener plugin`, `::ignores an internal link`, `::leaves a
  plain browser's link behavior alone`).

## Known gap

The goal panel, the session panel, the session detail view, the sessions
screen, the task card, the goal actions and the attention list
(`goal-panel.tsx`, `session-panel.tsx`, `session-detail-view.tsx`,
`sessions-page.tsx`, `task-card.tsx`, `goal-actions.tsx`, `attention.ts`)
have no `*.test.tsx`
or `*.test.ts` of their own: what they draw is exercised only incidentally,
through `detail-panels.test.tsx`, `pane-focus.test.tsx` and
`goal-swimlanes.test.tsx` mounting the screens that hold them. Rule 19's
account of the sessions screen in particular rests on reading
`sessions-page.tsx` rather than on a test asserting it.

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
