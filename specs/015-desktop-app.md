---
id: desktop-app
status: current
updated: 2026-11-14
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10, 66240a5a]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/features/**/*.test.ts
  - ui/src/routes/**/*.test.tsx
  - ui/src/api/**/*.test.ts
  - ui/src/components/**/*.test.tsx
  - ui/src/hooks/**/*.test.tsx
  - ui/src/lib/**/*.test.ts
  - ui/src/lib/tauri-config.test.ts
  - ui/src/telemetry/provider.test.tsx
  - ui/src/telemetry/telemetry.test.ts
  - ui/dashboard/src/**/*.test.ts
  - ui/dashboard/src/**/*.test.tsx
  - ui/dashboard/scripts/**/*.test.mjs
---

# Desktop app

Ariadne Desktop: the same daemon, driven from a window. A Tauri shell around
a React app that talks to the daemon over HTTP and follows one SSE stream. It
reaches only a loopback TCP listener: the Tauri origins and Vite origins are
allowed by default, and another browser origin needs `tcp_allowed_origins`
under 012.

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
   On Linux (WebKitGTK) every scroll area gets a thin scrollbar in theme
   colours, light and dark, from one rule in `index.css` keyed on a
   `data-platform="linux"` marker; macOS keeps its overlay scrollbars.
3. Screens: the goals board (swimlanes) has Active,
   All and Finished status segments. A status-menu icon holds a custom
   selection. Each goal is one slim lane under two global column headers; its
   one-line row carries the goal's tokens at the right end (rule 39). The task
   panel (facts, messages listed oldest first with both agents named by skills,
   diff, history), sessions — Ariadne's own,
   each shown in its console — skills, repositories, the agents of
   the daemon's ACP registry with their launch flags and the models each may
   be staffed on, Workflows — shipped and user workflows grouped separately,
   with a document editor and a parsed preview drawn as a numbered pipeline
   — Permissions — the AI permission model's settings behind the `ai`
   permission mode (022) — Dashboards
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
   `skills:`, `ranks:` and `gate:`; after `ranks:`, or after a comma in its
   capability band, it offers each eligible `fast`, `balanced`, `frontier` or
   `local` rank that the line does not already name. A capability band has no
   preference order. After `gate:` it offers `committed`, `pushed`, `merged`
   and `request-merged`; and after `skills:`, or after a comma in its list, it
   offers the skill catalog's names with each one's summary as the detail,
   leaving out `orchestration`, `pr-reviewer` and a skill named anywhere else
   on the same line, on either side of the cursor. Hovering a key, or a rank
   or gate value, shows one sentence on what it means, right up to the
   character before the value starts even with no space after the colon;
   hovering a skill name shows its summary, or that the catalog has none by
   that name
   (`workflow-help.test.ts::offers the body keys at the start of a body
   line`, `::offers the rank words after ranks:`, `::offers rank words after
   a comma, excluding the one the line already names`, `::excludes a rank
   already named later on the line, inserting before it`, `::ends a rank
   completion at the end of the token the cursor sits inside, in the first
   segment`, `::ends a rank completion at the end of the token the cursor
   sits inside, in a middle segment`, `::ends a rank completion at the end
   of the token the cursor sits inside, in the last segment`, `::offers the
   gate words after gate:`, `::offers skill names after skills:, with each
   summary as the detail`, `::never offers orchestration or pr-reviewer`,
   `::offers skill names after a comma, excluding the one the line already
   names`, `::excludes a skill already named later on the line, inserting
   before it`, `::excludes skills named on both sides, inserting between
   them`, `::explains the skills key`, `::explains the ranks key`,
   `::explains the gate key`, `::explains the rank value frontier`,
   `::explains the rank value balanced`, `::explains the rank value fast`,
   `::explains the rank value local`, `::explains a rank value named among
   others on the same ranks: line`, `::explains the gate value committed`,
   `::explains the gate value pushed`, `::explains the gate value merged`,
   `::explains the gate value request-merged`, `::explains a rank value
   directly after the colon, with no space`, `::explains a gate value
   directly after the colon, with no space`, `::explains a skill name
   directly after the colon, with no space`, `::shows a known skill's
   summary`, `::says when the catalog has no such skill`). The preview draws
   each column as a numbered step on a vertical rail, a card holding its
   title, id, description, skills and every eligible rank in its capability
   band in that order, and ends the rail with a marker for the end of the
   task; a step's gate shows as a labelled chip on the connector below it,
   and a step with no gate leaves that connector bare
   (`workflow-preview.test.tsx::draws the steps as a numbered pipeline, in
   order, ending with a marker`, `::shows a card's title, id, description,
   skills and rank, in that order`, `::shows every rank of a step's
   capability band as its own badge`, `::shows a step's gate as a chip on
   its connector, and none where a step has no gate`, and `::shows a parser
   refusal at its line, and dims the last good preview`). `skills-page.test.tsx` proves the
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
6. The app exports its own logs and metrics to the selected daemon's OTLP HTTP
   routes. The resource names service `ariadne-desktop` and the app version.
   Settings keeps **Send app diagnostics to the daemon** on by default; off
   sends no app diagnostics. API duration uses the method and route template,
   never request paths or bodies. It also counts stream reconnects and route
   navigations. Console warnings, errors, browser errors, and unhandled
   rejections become log records without request or response bodies.
   Native shell log events at the shell's configured level also reach the
   desktop resource when diagnostics are on. Their target is `tauri`.
   Development event payload debug lines are outside telemetry.
7. Query keys follow one convention, so an event knows which caches it
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
14. The model picker is one popover of three columns: agent, model and
    effort. A search box and a "Show disabled" switch sit above them (011).
    The agent column lists each agent that has a shown model. Each row has
    the agent's icon and id, in the order the catalog first names the agent.
    A pick there fills the model column with the models of that agent. Each
    row has the model id and its description. A pick there fills the effort
    column with `auto (<default>)` and the model's efforts, with their
    descriptions. A pick there completes the choice and closes the popover.
    The popover opens on the current pin, with its agent, model and effort
    selected. The trigger shows the agent icon, the model and the effort. It
    shows "Choose a model" when no model is chosen. A non-empty search
    replaces the agent and model columns with one list of the matching
    models. Each row has the agent icon, the model id and its description.
    A pick there clears the search and shows the columns on that agent and
    model. The picker takes no free-text model. A saved pin that is not in
    the catalog still shows on the trigger as it is. The arrow keys move
    within and across the columns, and Enter picks. Escape closes the
    popover and not the form dialog around it. No screen shows an automatic
    or default model; `auto` is an effort choice only.
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
    same tool and child fold into one, such as `Tool call · Read /file ×7`,
    which expands to the individual rows. A child's stored event prefixes its
    kind with the child's title (or id), such as `readiness_review · Tool call
    · Bash cargo test`; it never folds with a parent or another child. Its
    first `subagent_update` reads `Subagent started · <title>` and a later
    terminal update reads `Subagent <state> · <title>`, using its id where the
    title is empty. A running child has a pending dot, a completed one active,
    and a failed one danger. Each row still expands to its raw payload.
19. The sessions screen lists the sessions Ariadne has run in one table,
    newest activity first: the title, the status, the work — in one column,
    the seat a session holds as a badge, the goal it is under and the task
    under that, each of which narrows the table to its sessions when picked —
    the agent, the age and the tokens. It reads `GET /v1/sessions` whole,
    ended sessions included, and every row shows its working directory under
    its title. A filter bar above the table narrows the listing by agent —
    the registry of `GET /v1/acp-agents` — status, seat, directory, a since
    day, an until day, and a search over the titles; `?goal=` and `?task=`
    narrow it further, as a chip above the table. The search leads the bar,
    with the count at the other end of its row; under it each filter is one
    compact trigger naming what it filters and, once set, what it is set to,
    drawn dashed while unset. The directory and the activity window open a
    popover — the window with Today, Last 7 days and Last 30 days one click
    each, and an empty day shown as "Any day" rather than as the date WebKit
    fills an empty date field with — and Clear filters drops every filter of
    the bar in one step, leaving the goal and task scope. Each filter is a
    URL search param (`?agent=`, `?status=`, `?seat=`, `?dir=`, `?since=`,
    `?until=`, `?q=`), so a narrowed screen is what its URL says and opens
    again with those filters set; a typed one narrows the rows once the
    typing has settled rather than on every keystroke, and a day is compared
    as the moment that bounds it — the first instant of the day as `since`,
    its finest last moment as `until` (`23:59:59.999999999Z`), in UTC, so a
    day holds every session active on it. `status`, `seat`, `goal` and `task`
    reach `GET /v1/sessions`; `agent`, `dir`, `since`, `until` and `q` narrow
    its rows here. `status` and `seat` are remembered between visits; the
    rest are not, since they are for finding one session. The count line
    reads `<shown> of <total> sessions`. Picking a row opens its own panel
    (`?session=`). No control on the screen starts a session without a goal
    or a task.
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
    lost panel. The same holds for every read that has content cached: shared
    tables, the goals board, a goal's task list, and a task panel with its
    history and messages keep showing that content when a refresh fails, under
    one `RefreshWarning` ("Could not refresh …", marked as possibly outdated,
    with a Retry for that read alone). A read that never succeeded shows the
    load error instead. The warning leaves with the next successful refresh;
    actions, confirmations and their error feedback are untouched. The goal, task and session panels open on a dense fact list
    above their tabs — `text-xs`, three columns at `sm` and four at `lg`, no
    card frame — rather than the framed grid further down an entity's own
    screen; a stepped task's own workflow section is rule 40's.
21. Every session is shown in its console, a transcript the app draws from
    the daemon's console stream (`GET /v1/sessions/{id}/console/stream`,
    008). A `snapshot` replaces the transcript, and each `event` extends it.
    The pane's state, stream, scroll and keys are `console/session-console.tsx`;
    the blocks are drawn by `console/blocks.tsx`.
    The events fold into blocks as the CLI's transcript folds them: a user
    prompt, with a daemon prompt (`source: "daemon"`) marked as such; the
    agent's message as markdown; a thought as dimmed markdown; a plan's
    entries with their status; one tool call block per `tool_call_id`,
    merged across `pre_tool_use`, `tool_call_update` and `post_tool_use`,
    with its status, kind, title, output text and diff; a system note for a
    session's start and end, a cancelled or other non-`end_turn` stop,
    compaction and an `agent_request` hint; an error, with the note of the
    `session.diagnosis` whose `error_event_id` names it; and `kind · summary`
    for a kind it does not know. A person's prompt, an unfolded daemon prompt
    and a question card's text are Markdown, drawn by `components/markdown.tsx`
    as a message is: `h1` to `h3` in a visible scale above body text, lists
    with their nesting, task-list checkboxes, `del`, `kbd` and inline code
    styled, a single newline in a prompt still a line break, and a fenced
    block a `CodeBlock`. Fold summaries and the hint stay plain text, and
    `blocksText` keeps a prompt's raw text. Live `agent_message_chunk` and
    `agent_thought_chunk` events build one open block per speaker — the
    session or one subagent — and that speaker's stored `agent_message` or
    `agent_thought` closes it without repeating its text.
    Every block also keeps the `created_at` of its first event. A tool call
    block carries what a reader needs to read it: its locations, its start and
    end time, a subject derived as the CLI's pane does (the command's first
    line, `path:line`, `pattern in path`, the URL, else the title), its input
    as the whole command or as two-space JSON, and its output formatted — an
    object or JSON text pretty-printed and marked `json`, a `read`'s numbered
    fence and line prefixes removed with the first line number kept, and the
    language of a `read` taken from its file's extension; an image is one
    `[image]` line. `session.switched` and `session.pin_fallback` are notes
    with their summary, and a `usage_update` draws no block.
    A block keeps its first event id, and a subagent's block is tagged with
    the child's title. The transcript follows the newest block while the
    reader is at the bottom of it.
    A scroll to the top of the transcript loads the events older than the
    ones shown: one page of
    `GET /v1/events?session=<id>&order=desc&before=<oldest event id>&limit=200`,
    folded in before the blocks, with the reader's scroll position kept. A
    call whose `pre_tool_use` is in that page and whose later events are
    already shown is one block. The pane shows a loading line while a page
    loads, and an error with a Retry where the fetch fails. An empty page
    marks the start of the session, and nothing more is fetched until a
    fresh snapshot replaces the transcript, which also drops the pages
    loaded.
22. An input box posts a prompt as `{"text"}` to
    `POST /v1/sessions/{id}/console/input`: Enter sends it, and Shift+Enter
    adds a line. A failed post is kept, rule 22d. The
    box is disabled, with the reason, while the session is not live. A
    running turn shows a Stop control that posts
    `POST /v1/sessions/{id}/console/cancel` with no body, and Escape inside
    the console cancels it too. A `permission_request` with no
    `permission.replied` yet shows the call, its command or diff, and its
    options as a picker: Up, Down or 1 to 9 select an option, Enter or a
    click answers, and the answer is posted as `{"text": "<optionId>"}` to
    the same input route. The reply shows the chosen option and who decided
    (`decided_by`).
22b. The composer (`console/composer.tsx`) starts at one line and grows with
    newlines and pasted text to six lines, then scrolls inside. Send is an icon
    button with a tooltip at its right edge, aligned with its last line and never
    wrapping under it, disabled while the draft is blank or the session is not
    live, and Stop is one beside it while a turn runs. Enter sends and Shift+Enter adds a
    line; an Enter that ends an input-method composition does neither, and a
    pasted text keeps its lines.
22c. While a question card (rule 22a) waits for an answer, the composer
    answers the newest such question, one restored from the pending list
    included: its text posts as `{"text"}` to that
    request's own answer route and never to the input route, and shows no
    `queued` block, and the delivery prompt that follows draws none (rule
    22a). The card then shows `Answer sent`, and the next message
    goes to the input route. A permission pick and a normal message keep the
    input route.
22d. A message the daemon refuses leaves the queue and stays in the
    transcript with the error, a Retry and an Edit. Retry sends the same
    text again, to the same request where it answered a question, and leaves
    the draft as it is. Edit takes the text back into the composer in front
    of any newer draft, and gives the composer the keyboard. Refused messages are
    retained with the console's state (rule 24i), so Expand, Collapse and a
    change of tab keep them, and a send that fails after the pane was
    replaced shows in the pane now mounted.
22e. A plus menu beside the composer searches, by name and description, the
    agent's commands (rule 23b) and these actions: Switch agent/model, where
    the session's goal is not cancelled; Resume, only while the session is
    not live; Expand all or Collapse all, which also clears every block's
    own fold; and `Copy loaded transcript (<view>)`, which copies the blocks
    the console has loaded for the view shown, `main` or the child's title,
    as text and nothing older. Switch and Resume are the session actions of
    the session panel (`session-actions.tsx`), with the same dialog, toasts
    and navigation to the successor or the revived session. A command picked
    from the menu goes into the composer for editing in front of any draft,
    with the caret after it, and is never sent. After a pick the composer
    has the keyboard. Every entry is a button, so a click, a tap and the
    keyboard reach it: the search field takes the keyboard on opening, Down
    walks the entries, Up returns to the field and Enter in the field picks
    the first match. A long command name or copy label is cut with an
    ellipsis, whole on hover, and the list never scrolls sideways. The menu also shows in a child's view, with the
    actions alone.
22a. An `agent_request` event that carries a `question` (031 rule 11) is a
    question card (`console/questions.tsx`): the hint, the question text,
    `Recommended: <label>`, one button per option with its description, and
    a field for a custom answer. A click on an option submits at once.
    Every answer posts `{"option_id"}` or `{"text"}` to
    `POST /v1/sessions/{id}/agent-requests/{request_id}/answer` for the
    card's own request, never to the input route. A failed post shows its
    error on the card, which stays answerable for another attempt; an
    accepted one shows `Answer sent` until the request's
    `agent_request.resolved` event closes the card as `Answered · <answer>`,
    `Withdrawn by the agent` or `Expired`. A closed card, a card replayed
    from history and a card of a session that is not live take no answer.
    The prompt that delivers the answer to the agent (a `user_prompt_submit`
    that carries a `request_id`, 031 rule 12) draws no prompt block, and
    `blocksText` leaves it out, so an answered question shows once, as its
    card; the CLI attach pane still draws it. The question text is Markdown
    (rule 21).
    A hint-only request stays the note `waiting for you · <hint>`.
    With every snapshot, a reconnect's included, the console reads the
    session's pending requests (`GET /v1/sessions/{id}/agent-requests`) and
    draws a card, after the transcript, for each pending question the
    transcript neither shows nor has seen closed — one asked before the
    snapshot's page — so a reload never hides it; an older page that brings
    its line in draws it in place instead. A failed read shows its error
    with a Retry that reads the list again.
23. The console uses the app's own tokens, in both themes. An Expand control
    opens it in a near-fullscreen modal, and its Collapse control restores
    it to the panel; each move closes the old stream and opens a fresh one.
    Escape inside the console belongs to the console in the panel and in
    the modal alike; Escape outside the console closes the panel or the
    modal. ⌘Escape leaves the console without cancelling or closing
    anything itself: it moves focus to the panel's Close button, or to the
    modal's Collapse button, so the plain Escape that follows closes what
    is now focused.
    The console has the extras of the CLI's attach pane
    (`crates/ariadne-console/src/tui/`), rules 23a to 23f.
23a. A tool call with an input, an output or a diff, a thought and a daemon
    prompt each fold into one summary line: `thought ·` and the thought's
    first line; `daemon prompt ·`, its first line and its line count. A tool
    call's summary shows at its left the status mark, one icon per ACP kind
    (`read`, `edit`, `delete`, `move`, `search`, `execute`, `think`, `fetch`,
    and one more for any other kind) and the subject (rule 21). At its right
    end, muted, it shows the output's line count, the duration once the call
    has ended (`340ms`, `2.1s`, `1m 04s`) and the call's time as `HH:MM:SS`.
    A tool call is one frame. Unfolded, it shows its input, its output and
    its diff, in that order, each bare under a muted caption line (rule
    23a2): `input`, `output` or `diff`, then the language label where one is
    known. A `read` shows no input, because its subject names the file. A
    `read` output with a first line number has a line-number gutter that
    starts there, and the numbers are not part of the selected or copied
    text. A tool call's header keeps one line: text that does not fit ends
    in an ellipsis, the facts at the right end stay visible, and a hover on
    the header shows the full text. The body shows the full subject above
    the input, unless the input already holds it (a shell command's input
    is the command). A call with no input, output or diff is a plain line
    until its header is cut; then it folds, and its body is the full
    subject. A permission's header (`Permission ·` and the call) keeps one
    line the same way, with its time at the right end. A pending permission
    shows its header, command, diff, AI note and picker open, whatever the
    fold. An answered permission folds to one line, its header and its
    answer (`decided by` where the payload names one); a click unfolds the
    command, the diff and the AI note, and the choice is kept per block
    like the others. The AI note is the TUI's (`AI said ask · allow 62%,
    deny 5% · danger 21% (allow up to 5%, deny from 65%)`), shown where the
    request carries one. A permission shows its command bare under
    `command · Shell`.
    Every block shows its time: a muted `HH:MM:SS` at the right of a summary
    line and of a prompt, and the full date and time on hover of any block.
    `BlockView` is memoized, and it compares a question card or a permission
    picker by its props, so a streamed chunk re-renders only the block it
    changes. A card or a picker whose own state changes renders again. A message, a person's prompt, a plan and a
    question never fold, and a pending permission never folds. An unfolded daemon prompt is
    Markdown, and its summary line stays plain text. The whole summary is one button
    with `aria-expanded`, so a click, Enter or Space opens that block alone;
    its content sits outside the button, so selecting it, a link in it and
    a Copy control never toggle it. A block's own choice is a fold
    preference kept under its id (24i) and survives stream updates, the view
    changing and a remount. One toggle serves the whole console: a control
    in the console's header and Ctrl-O from anywhere in the console, the
    header's controls included, flip every block and clear the individual
    choices, and the control names the state it flips to: `Unfold` while
    folded, `Fold` while unfolded. The console starts folded on each open.
23a1. The transcript is the console's only scroll box. A message, a tool's
    output, a command, a diff, a table, a thought and a question card grow
    with their content and wrap a wide line; none has an `overflow` or a
    `max-height` of its own. The composer keeps its own six-line cap.
23a2. Code is a `CodeBlock` (`components/code-block.tsx`), in the Markdown
    renderer's fenced blocks and in the console's tool output, commands and
    diffs. `lib/highlight.ts` resolves a fence's name, or the extension of
    the file a `read` tool names, to a language of
    `@codemirror/language-data`. The block shows that language's label and
    parses the code with its CodeMirror grammar. The tokens take the light
    and dark highlight styles of `lib/highlight-style.ts`. The console's code
    and the task diff viewer share this module, so they share one colour
    scheme. A grammar loads on demand: the block
    shows the plain text first and the same text highlighted once the
    grammar has loaded. The tokens of one text, language and theme are
    computed once. An unknown language is plain text with no label. A diff
    keeps the green added and red removed lines, and a hunk header muted.
    Code has one frame, never a frame inside a frame. A fenced block in a
    message or a thought keeps one light frame, with its language label and
    an icon-only Copy on one caption line, and no header bar. Inside a tool
    call or a permission, the block's `caption` prop names the section: the
    code then has no border, background or header bar of its own, and its
    caption line ends with the same icon-only Copy. Copy puts the section's
    original text on the clipboard and never toggles a fold.
23a3. A reader who scrolls more than a few pixels from the newest output
    stops following and gets a `Jump to latest` button over the transcript,
    marked `New output` once the transcript changed since. The button
    follows the newest output again and clears the mark; returning to the
    bottom by scrolling does too. Streaming, older pages loading and folds
    leave the offset of a reader who scrolled away, and keep a following
    reader at the newest output, a resized panel too.
23b. The latest `available_commands_update` event replaces the command
    list, whole: a later list, an empty one included, replaces the earlier
    one. A list whose payload carries a `subagent`, of any value, is ignored, and the event
    draws no block.
23c. While the input starts with `/` and the cursor is in its first word, a
    suggestion list shows at most 8 of the listed commands whose name holds
    the typed text, each with its name, description and input hint, or says
    "no matching command". No list shows while a permission question waits.
    Up and Down move the pick, Enter or a click or tap on an entry completes
    it into the input, replacing only the command's first word and keeping
    the text after it, keeps the input's keyboard, and Tab keeps
    its browser meaning. Escape closes the list without cancelling the turn;
    the next edit of the input opens it again. A
    picked command is sent as plain text through `/console/input`, as every
    prompt is.
23d. A status row (`console/status.tsx`) above the input shows the session's status, and while a
    turn runs a spinner with `thinking` or `running <tool>` (the tool's first line,
    cut with an ellipsis, whole on hover) and a turn clock
    counted from the `created_at` of the turn's first event. It shows the
    token totals `↑in ↓out`: the sum over the launches of the latest
    `ariadne_usage` of each launch's `stop`, or the session's own usage
    where that is more, each count apart, at the right end. The row is one line at every width.
23e. A sent prompt shows at once as a user prompt block tagged `queued`.
    The `user_prompt_submit` with `source: "console"` and the same text,
    newer than the send, replaces it: an older echo, such as one a resync
    replays, does not. A failed post removes the queued block and keeps the message, rule 22d. Each send is its own queued block: a failed post removes the
    block of that send alone, even where an identical prompt is queued
    beside it, and where the echo of a later identical send came first, the
    failure clears the entry that echo left behind. A snapshot keeps a queued prompt it does not yet hold.
23f. A footer under the input names the keys of the state: idle
    (`enter send`), running (`esc cancel`) and a permission question
    (`up/down or 1-9 choose`, `enter answer`), each with the fold key.
24. A `resync` or a dropped stream is retried on the event stream's
    backoff, and the pane says it is reconnecting meanwhile. The fresh
    snapshot replaces the transcript, so no block shows twice.
24a. The console shows a session's subagents as the CLI attach pane does
    (008 rules 30 to 36; the bar is `ViewBar` in `console/status.tsx`). A `subagent_update` announces a child on its first
    `subagent.id`. A bar above the input shows only while the session has a
    child. It lists `main`, then each child in announcement order, with the
    mark of its state: `●` running, `✓` completed, `✗` failed, `⊘`
    cancelled, `◇` none. The child whose permission question waits carries
    `?`.
24b. A click on a bar entry switches the view. The bar is one Tab stop, on
    the entry shown: Left and Right move between the entries, in bar order
    and wrapping around, and Home and End go to the first and last; each
    shows its view and keeps the keyboard on the bar. Tab and Shift-Tab are
    the browser's.
24c. A child's view shows only the events that carry its
    `payload.subagent.id`. A call's update merges only into the block of
    the same speaker, even under the same call id or title. It has no input
    box and takes no typing. Its footer starts with `read-only`, then `arrows switch view`.
24d. Escape in a child's view returns to `main`. Where the child's latest
    `capabilities.cancel` is true and its state is `running`, Escape posts
    `{"subagent_id": "<id>"}` to `POST /v1/sessions/{id}/console/cancel`
    instead, and the footer says `esc cancel`, not `esc back`. A refusal,
    a `409` included, adds an error block to the child's view.
24e. The `main` view shows a line per child announcement,
    `◇ subagent <title> · <task>`, and a line per end state,
    `<mark> subagent <title> <state> · <elapsed>`, timed from the
    announcement. A child first heard of at its end gets both lines. A
    child's own events stay out of `main`.
24f. A child's `permission_request` and `permission.replied` show in
    `main`, and the question names the child:
    `<tool> · asked by subagent <title>`. The picker answers it from `main`.
24g. The status row says `running N subagents` while any child is in state
    `running`.
24h. A snapshot, the one a reconnect brings included, rebuilds the bar and
    the views. A shown child that the snapshot no longer holds gives way to
    `main`, once a snapshot has said which children there are.
24i. The console retains, in memory and per daemon address and session
    (`console/console-view-state.ts`): the draft, the transcript's scroll
    position and whether it follows the newest block, the view shown, the
    refused messages, the console's fold and a fold preference per block id. Expanding the
    console, collapsing it, and leaving the tab and coming back restore
    them; another session or another daemon starts with its own. A reader
    who scrolled up comes back to that place, not the bottom: the pane
    loads older pages back to the oldest event it had loaded, then restores
    the offset, so the place holds the same content. The console also
    retains the first block in view and where its top sat, and puts that
    block back there, so events that moved the page boundary do not move the
    place. A browser
    restart forgets them. Each console on screen holds one stream, and a
    hidden one holds none.
25. A session's view has two tabs over one space: the console, open by
    default, and the agent activity feed. The tab is in the URL (`?tab=`);
    the console's tab is `terminal` on the wire, so an older link still
    opens it. A tab value that is not one of the two opens the console.
    Leaving the console's tab closes its stream, and coming back opens a
    new one.
26. The sessions list shows the pin the session was launched on
    (`<agent>:<model>`): the row cuts the model id in the middle when needed,
    keeps the effort after an `@` readable, and its hint holds the whole pin
    (`ui/src/features/sessions/sessions-list.test.tsx::keeps the effort beside
    a middle-cut model and gives the row a whole-pin hint`).
27. A row of the Needs attention popover for an agent blocked on a permission or an
    input prompt opens that session's console with `?focus=`, so the
    console's input box takes the keyboard on arrival.
27a. The Needs attention popover has a View all action that opens a wider
    drawer over the same attention list, order and targets, and leaves the
    popover as it was
    (`ui/src/features/goals/attention-header.test.tsx::opens a drawer from View all and filters a goal-less item by goal`).
    Each row shows its whole subject, explanation, affected work and an action
    label that names where the row goes — console, task, session, pull request
    or repositories
    (`ui/src/features/goals/attention-header.test.tsx::labels a console row and a task row by where each one navigates`).
    Picking a row navigates to its target and closes the drawer
    (`ui/src/features/goals/attention-header.test.tsx::navigates to the row target from the drawer and closes it`).
    Goal and reason filters narrow one list, and "No goal" keeps goal-less
    items reachable
    (`ui/src/features/goals/attention-header.test.tsx::narrows the drawer list by reason and counts what is shown`).
    The drawer keeps items under a partial read and says so, and says when a
    read failed with nothing loaded
    (`ui/src/features/goals/attention-header.test.tsx::keeps items and reports a partial read in the drawer`,
    `ui/src/features/goals/attention-header.test.tsx::reports a failed read in the drawer when nothing loaded`).
    Closing it returns focus to the trigger
    (`ui/src/features/goals/attention-header.test.tsx::returns focus to the trigger when the drawer closes`).
28. Above the console, a session blocked on a permission or an input prompt
    shows a banner that says where to answer: the picker in the console for
    a permission, the console's input for a question. A session that has
    ended while blocked is told to resume first. No other attention reason
    shows the banner.
29. The typed keyboard chords (`n`, `g` then a letter, `?`, `[`) are ignored
    while a field, an editor or a session console's input box has the
    keyboard. The
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
    the fields scroll on a short window, with focus-ring padding and a
    reserved gutter so a Linux scrollbar covers no field;
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
37. The Dashboards screen is at `#/dashboards`, titled `Dashboards`, last in the sidebar
    (023). Its header holds a dashboard picker over `GET /v1/dashboards`
    (`qk.dashboards.list()`) and a Reset action; the rest of the screen is
    an iframe over a Perses dashboard, rendered by the `ui/dashboard`
    package. Picking a dashboard from the picker is kept in the URL as
    `?dashboard=`, defaulting to the shipped `default` dashboard, or the
    first the list answers with where that name is absent. Reset opens
    a confirm dialog (`ConfirmDialog`) whose confirmation sends
    `POST /v1/dashboards/{name}/reset`; the action itself is disabled while
    the picked dashboard's `document_is_default` is true, since there is
    nothing to go back to. The iframe's `src` is `/dashboard/`, queried with
    `daemon` (the configured daemon URL), `dashboard` (the picked name) and
    `theme` (the app's resolved light or dark theme, as it was the moment
    this `src` was last built) — same-origin in a packaged build, where it
    is this package's own build under the app's `dist/`, and in development,
    where the main dev server proxies `/dashboard` to this package's own dev
    server. `src` is rebuilt, reopening the iframe, only on the daemon or the
    picked dashboard changing — never on a later theme change alone, since
    assigning a new `src` navigates the iframe the same way a typed address
    would and would throw away whatever is open in Perses. Two
    `postMessage`s keep an already-open iframe in step instead:
    `{type: "theme", theme}` on every resolved-theme change after the first
    paint, and `{type: "reload"}` when the shown dashboard's own `updated_at`
    moves under it without the picker's own selection changing — an edit
    saved from another window or the CLI, landing here as a
    `dashboard_updated` event the dispatcher already patched into the row
    `qk.dashboards.detail(name)` (`dashboard_created` and `dashboard_updated`
    patch the detail and invalidate `qk.dashboards.lists()`;
    `dashboard_deleted` removes the detail and invalidates the lists too,
    reading the name off its own payload — the daemon sends the whole
    `DashboardDto` there as well, not the generic `DeletedDto` every other
    `*_deleted` event carries, since a dashboard has no id of its own for
    that to name). A
    `{type: "reload"}` has the iframe read the dashboard again and show that
    document in a fresh `ViewDashboard`, since Perses's own internal store
    only ever reads what it is given once, as its starting state, and a
    changed prop alone never reaches it. Inside the iframe, `ui/dashboard`'s
    own `Page` reads its three query parameters,
    supplies Perses's `DatasourceApi` with one global Prometheus and one
    global Loki datasource, each `directUrl` the daemon's own address — which
    serves the standard Prometheus API under `/api/v1/` and the standard
    Loki API under `/loki/api/v1/` at its root — loads
    `GET /v1/dashboards/{name}`, renders Perses's `ViewDashboard` in the
    view or edit mode spec 036 rule 10 decides, and saves an edit through
    `PUT /v1/dashboards/{name}` from `ViewDashboard`'s own `onSave`.
38. Toasts show at the bottom left, clear of the header and the pane
    (`ui/src/components/ui/sonner.test.tsx::renders toasts at the bottom left`).
39. The goals board shows one global "Tasks" heading with the number of
    visible tasks across listed goals, including folded goals. Each goal has
    one task area with a subtle dotted outline. Active hides finished and
    cancelled tasks; All, Finished and custom filters show every task in each
    matching goal. Progress counts use all tasks. Finished cards have green
    borders, cancelled cards gray borders, and failed cards red borders.
    Cards wrap into rows as needed. The goal's workflow steps make no columns.
    The lane's row is one line: a collapse
    chevron, the title as a link to `?goal=`, the goal status dot, `done/total`,
    the orchestrator attention badge, and the token figure at the right end.
    The title's tooltip gives the full title, each repository as
    `path [base_branch]`, the created time, and the progress as done against
    failed and stalled. The row has no progress bar and no repository
    summary. A collapsed lane shows the one-line summary text and no cells.
    Lanes keep their order: attention first, then live, then terminal goals.
    Each card stays one tab stop, and each task area is a named group
    (`<goal>: Tasks`).
    A task card is three contained rows, for every status, on the board
    and in the goal panel. Its target minimum width is 18rem (about 288px):
    the board fits fewer cards across, and a card shrinks inside a narrower
    panel. The card is a CSS `@container`, so its own width decides what is
    drawn, and no row overflows. Row 1 is the title (two lines with an
    ellipsis, long unbroken text wrapped), the session attention badge and
    one state badge: "Waits for N tasks" for a pending or ready task with
    dependencies, "Waits" without, "Failed" with the task's `reason` in its
    tooltip, "Stalled", or "Cancelled"; a task under way and not stalled shows
    none. Badges shrink to their dot on a narrow card, and keep their full
    text in tooltips and the accessible description. Row 2 is on one line for
    every status, finished and cancelled included: one circle per step of the
    goal joined by arrows, with a step's name beside it. A check is a step
    before the current one, a filled circle with `aria-current="step"` is the
    current one, a ring is one after it; a finished task shows every circle as
    a check and a task with no step every circle as a ring. The current step's
    name stays while the other names hide first as the card narrows; a
    workflow of more than five steps also folds the steps more than one from
    the current one behind an ellipsis. Each circle's tooltip names the step,
    its description, and its agent's model, effort and skills, with a link to
    the agent's session, or says "none staffed". Row 3 is anchored at the card bottom and is one line: the
    task's token figure (`↑input share ↓output`, including zero, rendered by
    the goal lane header's own `TokenFigure` at every width; its hover names
    Input and Output and lists one row per workflow step, and it is not a tab
    stop) at the left, last activity at the
    right, and, when the task has a `pr_url`, a compact PR link (icon and `#number`) in padding
    reserved for it. The time shortens ("3 minutes ago" to "3m") and truncate as the card narrows. Last
    activity is the latest of the task's `updated_at` and the
    `last_activity_at` of every session of that task, previous ones included
    (read from the shared sessions query, which the event stream already
    keeps fresh), and is `updated_at` when no session has activity. Model
    names appear only in tooltips and the accessible description, which also
    lists the whole workflow, the reasoning effort and per-agent usage. The
    card shows no branch, merge commit or dependency icon, is one link to
    `?task=` with one tab stop, and keeps its border order: finished, cancelled or failed status, attention,
    stalled. The goal panel's task list shows
    the same card (`ui/src/features/tasks/task-card.test.tsx`).
40. The task panel of a stepped task draws one workflow section under its
    facts, in place of a separate Agents fact and step strip: a track of
    nodes, one per column in order, and under each node the column's title,
    its agent's skills, model and effort, and a link to its live session —
    every column at once, nothing to open, and no column description. A
    node's state — done, current or pending — is a colour paired with an
    icon and a text label (a check, a pulsing dot, an empty ring; the label
    read to assistive technology), never colour alone, and the current node
    carries `aria-current="step"`. There is no author or reviewer fact. The
    history names a step move by its `from_step` and `to_step` columns.
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
    sessions screen's seat filter offers the seat.
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
45a. The task panel opens on an execution summary above the technical facts:
    a card with a headline beside the status dot, a muted detail line, and
    one row for each stall, session and dependency it names. It is built
    from recorded state alone: the status and step, the failure or
    cancel reason, the stalled flag, each unfinished dependency named by
    title and status in text and linked to its task, and each live session
    with an attention reason, linked to that session. A task with no
    recorded blocker says its cause is unknown; the summary never infers
    one from silence or elapsed time. Branch, worktree, tokens and the
    other identifiers stay in the facts below it
    (`ui/src/features/tasks/task-panel.test.tsx::explains a pending task by
    the dependencies it waits for`, `::says the cause is unknown for a
    pending task with nothing recorded`, `::does not claim a blocker while a
    dependency's status is unresolved`, `::lists unfinished dependencies of
    a failed task with its reason`, `::links a live session that waits for
    permission`). A dependency whose status has not loaded is not a
    blocker, and unfinished dependencies show in every non-finished status.
46. The packaged webview has a content security policy that permits scripts,
    styles, fonts and images from the app, and images from the ACP registry's CDN
    (`https://cdn.agentclientprotocol.com`) alone. It permits HTTP, HTTPS, WebSocket
    and secure WebSocket connections because the user selects the daemon's
    host. It permits Tauri IPC connections through `ipc:` and
    `http://ipc.localhost`, so bundled plugins can work. It permits a frame
    from the app itself (`frame-src 'self'`), which is what lets the Dashboards
    screen's iframe (rule 37) open at all. It permits no objects, bases,
    embedded ancestors or evaluated script.
47. A click on an external `<a>`, anywhere in the app, opens it in the
    system's default browser instead of the webview, which otherwise ignores
    `target="_blank"` and has no opener wired in. The handler permits only
    `http:`, `https:` and `mailto:` anchors; it stops every other scheme. It
    opens the target directly, with no confirmation. A link in the console's markdown is such an anchor. An in-app
    link, whose href resolves onto the app's own origin, is left alone.
    Outside Tauri — the Vite dev server, and every test — the handler does
    nothing and a plain browser's own link behavior applies.
48. While a goal is planning and has an orchestrator, its panel shows a
    planning guide between the facts and the tabs. The guide words only what
    the newest orchestrator session records: a `waiting_input` or `waiting_permission`
    reason (waiting for you) whatever the status, else `starting`, `running`
    (working), plain `idle`, or ended. A failed read shows unavailable even
    over cached sessions. With no session recorded, or while the
    sessions are loading or unreadable, it states that and claims no progress.
    The Open orchestrator conversation button appears only with a session and
    opens it over the panel as the Sessions tab does. Create task manually is
    the guide's secondary action. The goal tabs and the header's New task
    action stay as they were.

49. The goals screen shows only the board: one swimlane per goal, with no
    List view, no Board/List toggle and no repository filter. A title-only
    search combines with the status filters to narrow the board, and an empty
    search result says that nothing matches the search.

## Acceptance criteria

- The planning guide words the recorded orchestrator session, a recorded wait
  before its status, and opens it
  (`ui/src/features/goals/planning-guide.test.tsx::words a recorded %j session as %s`);
  it claims nothing without a session or after a failed read
  (`ui/src/features/goals/planning-guide.test.tsx::claims no progress when no session is recorded, and keeps manual task creation`,
  `ui/src/features/goals/planning-guide.test.tsx::says the state is unavailable when a refetch fails over cached sessions`,
  `ui/src/features/goals/planning-guide.test.tsx::says the state is unavailable when the sessions cannot be read`).
- Enabled desktop diagnostics export native shell events at their severity
  with a `tauri` target (`ui/src/telemetry/provider.test.tsx::forwards native shell logs at their level when diagnostics are enabled`,
  `ui/src/telemetry/telemetry.test.ts::exports operational info with its target and fields`).
- The workflow editor completes each comma-separated `ranks:` segment with
  every rank absent from its unordered capability band. The preview identifies
  every listed rank as an eligible model capability
  (`ui/src/features/workflows/workflow-help.test.ts::offers rank words after a comma, excluding the one the line already names`,
  `::excludes a rank already named later on the line, inserting before it`,
  `::ends a rank completion at the end of the token the cursor sits inside, in the first segment`,
  `::ends a rank completion at the end of the token the cursor sits inside, in a middle segment`,
  `::ends a rank completion at the end of the token the cursor sits inside, in the last segment`, and
  `ui/src/features/workflows/workflow-preview.test.tsx::shows every rank of a step's capability band as its own badge`).
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
- The Dashboards screen opens its iframe on the shipped `default` dashboard
  when the URL names none — an absent or empty `?dashboard=` alike — even
  where another dashboard sorts before it, falling back to the first the
  list answers with where `default` is absent, with the daemon, the
  dashboard's name and the resolved theme on the iframe's own query string,
  and reopens it on the dashboard a `?dashboard=` in the URL names
  (`ui/src/routes/dashboards.test.tsx::opens the iframe on the shipped
  default dashboard when the URL names none, even though custom sorts
  before it, with the daemon, its name and the theme on its own query
  string`, `::falls back to the first dashboard the list answers with where
  none is named default`, `::treats an empty dashboard parameter as naming
  none, and opens the default dashboard`, `::opens the dashboard the URL
  names`); the picker lists every dashboard and switches the iframe on a
  pick
  (`::lists every dashboard in the picker, and switches the iframe on a
  pick`); Reset is disabled on the shipped document and enabled once
  something was saved over it, and a confirmed reset sends
  `POST /v1/dashboards/{name}/reset`
  (`::disables Reset on the shipped document, and enables it once something
  was saved over it`, `::asks before it resets a dashboard, then sends the
  reset`); an edit saved elsewhere tells the iframe to load the document
  again, through the same `postMessage` the host sends on a theme change
  (`::tells the iframe to load the document again once the shown dashboard
  is edited elsewhere`); a theme change afterwards leaves the iframe's own
  `src` untouched, reaching it only as a `postMessage`
  (`::keeps the iframe's src unchanged across a theme change, and posts the
  new theme instead`).
- The dispatcher patches a created or updated dashboard into its detail and
  refetches the lists, and drops a deleted one the same way `skill_deleted`
  and `workflow_deleted` do
  (`ui/src/events/dispatch.test.ts::patches created and updated dashboards,
  then drops a deleted dashboard`).
- The packaged webview's content security policy permits a same-origin
  frame (`ui/src/lib/tauri-config.test.ts::carries a restrictive content
  security policy`).
- The `ui/dashboard` package's own glue is tested apart from Perses, which
  is too heavy to mount in jsdom: its `DatasourceApi` answers the daemon's
  own URL for a global Prometheus and a global Loki datasource and nothing
  project-scoped
  (`ui/dashboard/src/datasource-api.test.ts`); it reads `GET` and writes
  `PUT /v1/dashboards/{name}` against a stubbed daemon, surfacing the
  daemon's own refusal message
  (`ui/dashboard/src/dashboard-client.test.ts`); it reads `daemon`,
  `dashboard` and `theme` off its own query string, defaulting the theme to
  light
  (`ui/dashboard/src/page-params.test.ts`); and it answers a same-origin
  `{type: "theme"}` or `{type: "reload"}` message and ignores any other
  origin or shape, until it is unmounted
  (`ui/dashboard/src/use-host-messages.test.ts`).
- 97 test files cover the features, the API layer and the event stream of
  `ui/`, and the glue of `ui/dashboard`; most screens are asserted in their
  own `*.test.tsx` beside them, though a few (noted under Known gap) are
  exercised only through the panels and boards that mount them.
- The sidebar lists Goals, Sessions, Forge, Repositories, Workflows, Agents,
  Skills, Permissions and Dashboards, in that order, and `#/repositories` mounts
  the repositories screen, while a screen the app dropped leads nowhere
  (`ui/src/components/app-shell.test.tsx::ends the navigation with dashboards, and lists Forge beside the other screens`,
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
- Each agent's tab shows its icon: the SVG bundled for it, else the registry
  `icon` URL of `GET /v1/acp-agents`, else the first letter of its id. An image that
  fails to load hands over to the next source in that order, so a failed
  bundled SVG tries the registry URL, and a failed URL leaves the letter
  (`ui/src/features/agents/agent-icon.test.tsx::tries the registry URL when the bundled image fails, then the letter when it fails too`,
  `ui/src/features/agents/agents-page.test.tsx::draws each agent's icon beside its tab, from the registry URL where nothing is bundled`).
  The webview's `img-src` adds `https://cdn.agentclientprotocol.com` and no
  other host
  (`ui/src/lib/tauri-config.test.ts::lets images load from the ACP registry's CDN and no other host`).
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
- The picker's trigger shows the agent icon, the model and the effort, and
  asks for a model until one is chosen
  (`ui/src/features/models/pin-picker.test.tsx::shows the agent icon, the model and the effort on the trigger`,
  `::asks the user to choose a model before one is picked`).
- The popover puts a search box and a "Show disabled" switch above the
  agent, model and effort columns
  (`ui/src/features/models/pin-picker.test.tsx::puts a search box and a Show disabled switch above the agent, model and effort columns`).
  Which turned-off models it lists is spec 011's rule.
- The agent column lists each agent with a shown model, with its icon, in
  catalog order; an agent pick fills the model column with its models, each
  with its id and description
  (`ui/src/features/models/pin-picker.test.tsx::lists each agent with a shown model, with its icon, in catalog order`,
  `::fills the model column with the picked agent's models, each with its id and description`).
- A model pick fills the effort column with `auto (<default>)` and the
  model's efforts, and an effort pick completes the choice and closes the
  popover
  (`ui/src/features/models/pin-picker.test.tsx::fills the effort column from the picked model, and an effort pick completes the choice`,
  `::picks the agent's own effort as auto`).
- The popover opens on the current pin, with its agent, model and effort
  selected
  (`ui/src/features/models/pin-picker.test.tsx::opens on the current pin, with its agent, model and effort selected`).
- A search replaces the agent and model columns with one list of the
  matching models, matched by id or description, and a pick from it clears
  the search and shows the columns on that agent and model
  (`ui/src/features/models/pin-picker.test.tsx::replaces the agent and model columns with one list of every matching model, on and off`,
  `::finds a model by what its agent said about it`,
  `::clears the search on a pick and shows the columns on that agent and model`).
- The picker offers no free-text model, and a saved pin that is not in the
  catalog still shows on the trigger as it is
  (`ui/src/features/models/pin-picker.test.tsx::offers no free-text row, and keeps a saved pin the catalog does not carry on the trigger`).
- The picker works with the keyboard alone, and Escape closes the popover
  and not the form dialog around it
  (`ui/src/features/models/pin-picker.test.tsx::picks with the keyboard alone: search, arrows, enter`,
  `::moves across the columns with the arrow keys`,
  `::closes on Escape, and leaves the form dialog open`).
- Every form that pins a model picks it through this picker: the task form,
  the goal dialog, the switch dialog, and the review and
  babysit dialogs
  (`ui/src/features/tasks/task-form-dialog.test.tsx::pins a column's model and effort through the three-column picker`,
  `ui/src/features/issues/issues-page.test.tsx::submits a goal dialog opened from an issue with the issue's data and the model picked`,
  `ui/src/features/sessions/session-actions.test.tsx::opens the switch dialog, posts its pin, and follows a successor`,
  `ui/src/features/pull-requests/pull-request-panel.test.tsx::starts and stops a review of a Dependabot request without a review assignment`,
  `::starts and stops babysitting a Dependabot request without a review assignment`).
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
  (`ui/src/features/models/pin-picker.test.tsx::fills the effort column from the picked model, and an effort pick completes the choice`,
  `::takes free text for a model of a known agent the catalog does not list`,
  `::takes free text for a model nothing has discovered`).
- An effort the new model does not take is cleared, one it takes travels,
  and a model with no efforts says so
  (`ui/src/features/models/pin-picker.test.tsx::drops an effort the model moved to does not take`,
  `::keeps an effort the model moved to takes as well`,
  `::drops the effort of a model that takes none, and says so`,
  `::requires a model before it offers effort choices`).
- The agent activity feed shows the daemon's summary and opens and closes the
  raw payload under its row. It names a child start and terminal state, prefixes
  a child's stored events, keeps child and parent folds apart, and colors a
  running, completed, or failed child by state
  (`ui/src/features/sessions/session-activity.test.tsx::names subagent updates by their first and completed state`,
  `::prefixes child events and keeps their folds separate from other children`,
  `::colors running, completed, and failed subagents from their states`).
- The sessions screen lists every Ariadne session, and the agent, directory,
  since, until and search filters each narrow its rows; no control starts a
  session of its own
  (`ui/src/features/sessions/sessions-page.test.tsx::lists every Ariadne session when no filter is set`,
  `::narrows the rows to the sessions of one agent`,
  `::narrows the rows to the sessions under a directory`,
  `::narrows the rows to the sessions active since a day`,
  `::narrows the rows to the sessions active until a day`,
  `::narrows the rows to the titles a search matches, ignoring case`,
  `::starts no session of its own`).
- The console folds the events of the stream into its blocks: prompts, a
  daemon prompt marked, message and thought, a plan, one block per tool call
  with its output and diff, the system notes, an error with its diagnosis,
  an unknown kind as its summary, a subagent's tag and a block's first event id
  (`ui/src/features/sessions/console/transcript.test.ts::folds a prompt, marking one the daemon sent`,
  `::folds the agent's message and thought, and a plan with its statuses`,
  `::merges every event of one tool call into one block`,
  `::reads a tool call's diff from a patch or from its old and new text`,
  `::notes a session's start and end, a stop other than end_turn, compaction and a request`,
  `::reads a diagnosis onto the error it names, and an unknown kind as its summary`,
  `::tags a subagent's block with its id and title`,
  `::keeps a block's first event id`).
- Live chunks build one open block that the stored whole closes without
  repeating it, a late chunk of a closed turn is dropped, and a child's
  chunks and whole stay apart from the parent's
  (`ui/src/features/sessions/console/transcript.test.ts::builds one open block from live chunks, closed by the stored whole without repeating it`,
  `::drops a late chunk of a turn its whole already closed`,
  `::keeps a parent's and a child's interleaved %s runs apart`,
  `::leaves a parent's open block open when a child's whole arrives`).
- A permission shows its call, command and options, then its reply; one on
  Ariadne's own tool that the daemon allowed is dropped
  (`ui/src/features/sessions/console/transcript.test.ts::asks a permission with its call, command and options, then shows the reply`,
  `::drops a permission on Ariadne's own tool that the daemon allowed`).
- A snapshot replaces the transcript, an event it already holds is added
  once, and a turn runs from its prompt to its stop
  (`ui/src/features/sessions/console/transcript.test.ts::replaces the transcript with a snapshot, and adds an event it already holds once`,
  `::says a turn runs from its prompt until its stop`).
- The stream client reads the session's console stream, hands over the
  snapshot and each event, and reconnects on the backoff after a `resync`
  or a drop
  (`ui/src/features/sessions/console/console-stream.test.ts::reads the console stream of the session`,
  `::hands over the snapshot and every event after it`,
  `::reconnects on the backoff after a resync`,
  `::reconnects on the backoff after a drop, and stops for good on stop`).
- The console draws the snapshot, marks a daemon prompt, extends and
  replaces its transcript, says it reconnects and takes the fresh snapshot
  without duplicates, and closes its stream when it leaves the screen
  (`ui/src/features/sessions/console/session-console.test.tsx::reads the console stream and draws the snapshot as blocks`,
  `::marks a prompt the daemon sent`,
  `::extends the transcript with each event and replaces it with a new snapshot`,
  `::says it reconnects after a resync and takes the fresh snapshot without duplicates`,
  `::closes the stream when it leaves the screen`).
- A scroll to the top loads one older page and keeps the reader's place,
  merges a call across the page boundary into one block, shows loading and
  an error with a retry, stops at an empty page, and starts over on a fresh
  snapshot
  (`ui/src/features/sessions/console/session-console.test.tsx::loads one older page when scrolled to the top, and keeps the reader's place`,
  `::shows a loading state, then an error with a retry that loads the page`,
  `::stops fetching once an empty page marks the start, until a reconnect`,
  `::drops the loaded pages when a fresh snapshot replaces the transcript`,
  `ui/src/features/sessions/console/transcript.test.ts::merges a call whose start is in an older page with its end already shown`).
- The input sends on Enter and adds a line on Shift+Enter, a refused send
  adds an error block, and the box is disabled with a reason when the
  session is not live
  (`ui/src/features/sessions/console/session-console.test.tsx::sends a prompt on Enter, and Shift+Enter adds a line`,
  `::adds an error block when the daemon refuses the prompt`,
  `::disables the input with a reason when the session is not live`).
- A running turn is cancelled from its control and from Escape inside the
  console
  (`ui/src/features/sessions/console/session-console.test.tsx::cancels a running turn from its control and from Escape inside the console`).
- The bar shows only with a child, and lists `main` and each child in
  announcement order with the mark of its state and `?` on the child that
  asks (rule 24a)
  (`ui/src/features/sessions/console/session-console.test.tsx::shows the bar only with a child, listing main and each child with its mark`,
  `::marks a completed and a cancelled child`,
  `ui/src/features/sessions/console/transcript.test.ts::lists each child in announcement order with its state and whether it may be cancelled`).
- The arrow keys switch the view inside the bar, which is one Tab stop, and
  a click does too; Tab stays the browser's (rule 24b)
  (`ui/src/features/sessions/console/session-console.test.tsx::switches the view with the arrow keys inside the bar and with a click, wrapping around`,
  `::keeps Tab for the browser, and the view bar a single stop in the Tab order`).
- The composer grows to six lines, sends from Enter and from Send, and keeps
  a newline, a composition and a paste (rule 22b)
  (`ui/src/features/sessions/console/composer.test.tsx::starts at one line, grows with its text to six lines, then scrolls inside`,
  `::sends with Enter and the Send control, and keeps a newline, a composition and a paste`,
  `::offers Stop beside Send while a turn runs`,
  `::draws Send and Stop as icons without a label, on a row that never wraps`).
- The prompt that delivers an answer draws no block (rule 22a)
  (`ui/src/features/sessions/console/transcript.test.ts::draws no prompt block for the prompt that delivers an answer`).
- The composer answers an open question on its request's own route (rule 22c)
  (`ui/src/features/sessions/console/composer.test.tsx::answers an open question from the composer on that request's own route`,
  `::answers a question restored from the requests list from the composer`,
  `::answers the newest of a restored and a loaded question`).
- A refused message keeps its text with Retry and Edit, and the newer draft
  (rule 22d)
  (`ui/src/features/sessions/console/composer.test.tsx::keeps a refused message with Retry, and Retry sends the same text without touching the draft`,
  `::returns a refused message for editing in front of the newer draft`,
  `::keeps a refused message, with Retry and Edit, when the console is unmounted and mounted again`,
  `::shows in the replacement pane a send that fails after the console was expanded`).
- A suggestion completes the command's first word and keeps the text after it
  (rule 23c)
  (`ui/src/features/sessions/console/composer.test.tsx::completes a suggestion by keyboard or click and keeps the text after the command`).
- The plus menu inserts a command for editing, offers Resume only when the
  session ended and Switch only where it can, expands and collapses all, and
  copies the loaded transcript of the view shown (rule 22e)
  (`ui/src/features/sessions/console/composer.test.tsx::cuts a long command name and the copy label in the menu, whole on hover, with no sideways scroll`,
  `::inserts a menu command in front of the draft for editing, and hands the composer focus`,
  `::picks a menu entry with the keyboard`,
  `::completes a suggestion on a click, keeping the composer's focus`,
  `::offers Resume only once the session has ended, and resumes through the existing route`,
  `::hides Resume while the session is live, and Switch where it cannot switch`,
  `::keeps a draft of only spaces and newlines when a menu command is inserted`,
  `::opens the switch dialog from the menu`,
  `::copies only the loaded transcript of the view shown, and names that scope`,
  `::expands and collapses everything from the menu`).
- The console folds tool details, thoughts and daemon prompts into summaries,
  each on its own header, keeps the choice, and draws no nested scroll area
  (rules 23a to 23a3)
  (`ui/src/features/sessions/console/session-console.test.tsx::folds a prompt the daemon sent into a summary that names it`,
  `::folds tool details, thoughts and daemon prompts together, starting folded, from the header and Ctrl-O`,
  `::folds one block on its own header and keeps the choice across a remount and a stream update`,
  `::lets a keyboard open a fold and leaves its content's Copy control from folding it`,
  `::draws no scroll area inside the transcript, and wraps long code`,
  `::labels and highlights a fenced block of a known language, and leaves an unknown one plain`,
  `::copies the code as written, not the highlighted markup`,
  `::offers Jump to latest with an unread marker once the reader scrolls away`,
  `::keeps the block the reader is on in place when a block above it changes height`,
  `::copies a fenced block's trailing blank lines as written`,
  `ui/src/lib/highlight.test.ts::names a language by its fence or its extension, and labels it`,
  `::leaves a name the data does not know, or plain text, unknown`,
  `::reads an extension that is not only letters and digits`,
  `::leaves a fence named after an object property unknown`,
  `::loses no text`,
  `::colours a JSON property name and a string value differently`,
  `::colours Markdown headings, emphasis, links and fences`,
  `::colours HTML tags and attributes`,
  `::keeps a Python triple-quoted string as one string token`,
  `::keeps a shell heredoc as one string token`,
  `::colour by the light and the dark style of the task diff viewer`,
  `ui/src/components/code-block.test.tsx::shows the plain text first and the same text highlighted once the grammar loads`,
  `::shows a language the data does not know as plain text with no label`,
  `::shows a fence named after an object property as plain text`,
  `::keeps the tints of a diff's added, removed and hunk-header lines`,
  `::draws a fence in one light frame, its label and an icon-only Copy on one caption line`,
  `::draws a captioned block bare, under a caption that names the section and the language`).
- A tool call is one frame with its input, output and diff under captions,
  a summary of status, kind, subject, count, duration and time, and every
  block shows its time; `BlockView` re-renders only the block a chunk
  changes (rules 23a and 23a2)
  (`ui/src/features/sessions/console/blocks.test.tsx::draws a tool call in one frame, with its input, output and diff bare under their captions`,
  `::copies one section's text from its icon-only control and never toggles the fold`,
  `::names a call's status, kind and subject at the left of its summary, and its count, duration and time at the right`,
  `::gives every ACP kind its own icon and any other kind one more`,
  `::says how long a call took in the unit that fits it, once it has ended`,
  `::shows a read's output with no input, in its file's language, numbered from its first line outside the text`,
  `::shows a permission's command as shell`,
  `::shows every block's time: the clock on a prompt and a summary line, and the full date on hover`,
  `::re-renders only the block a streamed chunk changes`,
  `ui/src/features/sessions/console/session-console-renders.test.tsx::draws a question card again for its own change, not for a message streamed after it`,
  `::draws a permission picker again for its own change, not for a message streamed after it`).
- The console retains a session's draft, fold and place apart per daemon and
  session, and keeps one open stream while shown (rule 24i)
  (`ui/src/features/sessions/console/composer.test.tsx::brings a session's draft and fold back after the console is unmounted`,
  `::keeps each daemon's and each session's state apart`,
  `::restores the reader's place, not the bottom, when the console comes back`,
  `::brings back the older page the reader had loaded, and their place in it, after the console expands`,
  `::puts the block the reader was on back in place when new events move the page boundary`,
  `::holds one open stream for the console shown, and none once it is hidden`).
- A child's view shows its own events only, has no input and says
  `read-only` (rule 24c)
  (`ui/src/features/sessions/console/session-console.test.tsx::shows a child's own events read-only, and its questions and lines in main`,
  `ui/src/features/sessions/console/transcript.test.ts::shows a child's events in its own view only`,
  `::keeps a parent's and each child's tool calls apart with %s`).
- Escape in a child's view goes back to `main`, or cancels a running child
  that may be cancelled, and a refusal shows in the child's view (rule 24d)
  (`ui/src/features/sessions/console/session-console.test.tsx::goes back to main on Escape from a child that may not be cancelled`,
  `::cancels a running child that may be cancelled on Escape, and shows a refusal in its view`).
- `main` has a line per announcement and per end, a child first heard of
  at its end has both, and a child's events stay out (rule 24e)
  (`ui/src/features/sessions/console/transcript.test.ts::gives main a line per announcement and per end, timed from the announcement`,
  `::gives a child first heard of at its end both lines`,
  `ui/src/features/sessions/console/session-console.test.tsx::shows a child's own events read-only, and its questions and lines in main`).
- A child's question shows in `main`, names the child, and is answered
  there (rule 24f)
  (`ui/src/features/sessions/console/transcript.test.ts::asks a child's permission in main, naming the child, and answers it there`,
  `ui/src/features/sessions/console/session-console.test.tsx::shows a child's own events read-only, and its questions and lines in main`).
- The status row counts the running subagents (rule 24g)
  (`ui/src/features/sessions/console/session-console.test.tsx::says how many subagents run`).
- A fresh snapshot rebuilds the bar and the views (rule 24h)
  (`ui/src/features/sessions/console/session-console.test.tsx::rebuilds the bar and the views from the snapshot of a reconnect`,
  `ui/src/features/sessions/console/transcript.test.ts::rebuilds the children from a fresh snapshot`).
- A permission is answered from its picker with the keys or a click, and
  the reply shows who decided; Shift+Enter and a composing Enter in an
  empty input answer nothing
  (`ui/src/features/sessions/console/session-console.test.tsx::answers a permission from its picker with the keyboard, and shows who decided`,
  `::picks a permission option with a click`,
  `::answers no permission on %s from an empty input`).
- A prompt, an unfolded daemon prompt and a question's text render as
  Markdown, with a single newline kept and a fence a `CodeBlock` (rules 21,
  22a and 23a)
  (`ui/src/features/sessions/console/blocks.test.tsx::renders a prompt as Markdown: headings, task lists, a fenced block, and a single newline still breaks`,
  `::renders an unfolded daemon prompt as Markdown and keeps its summary plain`,
  `::draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text`,
  `ui/src/features/sessions/console/session-console.test.tsx::renders a question's text as Markdown, with a fence as a code block`).
- A question is a card whose clicked choice posts at once to its own
  request's answer route; a failed post shows on the card and a retry
  succeeds; a custom answer posts as text; an answered card takes no answer
  (rule 22a)
  (`ui/src/features/sessions/console/session-console.test.tsx::draws a question as a card and submits a clicked choice at once to its request`,
  `::shows a failed answer on the card and keeps it answerable for another attempt`,
  `::submits a custom answer as text`,
  `::draws an answered question closed, with no choice to submit again`,
  `::restores a pending question older than the snapshot page on a load and a reconnect`,
  `::draws a pending question once where the snapshot already holds it, and closed where it closed`,
  `::shows a failed read of the pending questions, and its Retry restores them`),
  and the transcript folds a question and its resolution into one card,
  keeps a hint-only request a note, and replays a withdrawn question closed
  (`ui/src/features/sessions/console/transcript.test.ts::folds a structured question into one card that its resolution closes`,
  `::keeps a hint-only request a note, and a resolution of it adds no block`,
  `::replays a withdrawn question closed, so it is never answerable again`).
- The console expands into a near-fullscreen modal and collapses back on a
  fresh stream, keeps the modal open on Escape inside it and closes the
  modal on Escape outside it, and ⌘Escape moves focus to Collapse
  (`ui/src/features/sessions/console/session-console.test.tsx::expands into a near-fullscreen modal and collapses back on a fresh stream`,
  `::keeps the modal open on Escape inside the console, and closes it on Escape outside`,
  `::moves focus to Collapse on ⌘Escape and cancels nothing`,
  `ui/src/components/panel-sheet.test.tsx::leaves Escape on the board alone and returns focus when the pane closes`).
- The input takes the keyboard on arrival when asked, and the transcript
  follows the newest block only while the reader is at the bottom
  (`ui/src/features/sessions/console/session-console.test.tsx::gives the input the keyboard when asked to on arrival`,
  `::follows the newest block while the reader is at the bottom`).
- The fold toggle folds output, diffs and daemon prompts together, starts
  folded, and flips from the header control and from Ctrl-O
  (`ui/src/features/sessions/console/session-console.test.tsx::folds output, diffs and daemon prompts together, starting folded, from the header and Ctrl-O`,
  `::folds on Ctrl-O from a header control, not only from the input`).
- The latest command list replaces the earlier one, an empty list and all,
  and a subagent's list is ignored
  (`ui/src/features/sessions/console/transcript.test.ts::replaces the command list with each later one, an empty list included`,
  `::ignores a command list a subagent sent`,
  `::ignores a command list whose subagent is %s`).
- The suggestion list shows the matching commands, at most eight, says none
  matches, completes the pick, closes on Escape without cancelling, and a
  command goes as text
  (`ui/src/features/sessions/console/session-console.test.tsx::suggests the agent's commands while the first word is typed, and completes the pick`,
  `::says no command matches, closes the list on Escape without cancelling, and sends a command as text`,
  `::shows at most eight suggestions`,
  `::reopens the suggestions on the next edit after Escape closed them`).
- The status row shows the status, the turn with its clock and the token
  totals
  (`ui/src/features/sessions/console/session-console.test.tsx::shows the status, the turn with its clock, and the token totals`,
  `ui/src/features/sessions/console/transcript.test.ts::totals the latest usage of each launch, never below the session's own`,
  `::starts the turn clock at the turn's first event and names the open tool`,
  `ui/src/features/sessions/console/status.test.tsx::shows only the first line of a multiline tool name, whole on hover, with the tokens at the right end`).
- A sent prompt shows as queued until its console echo replaces it, and a
  failed send removes it
  (`ui/src/features/sessions/console/session-console.test.tsx::shows a sent prompt as queued until the console echo replaces it`,
  `::removes the queued prompt and adds an error block when the send fails`,
  `ui/src/features/sessions/console/transcript.test.ts::shows a queued prompt until its console echo arrives, and drops it on a failed send`,
  `::keeps a queued prompt across a snapshot that does not hold its echo`,
  `::matches a queued prompt only to an echo newer than its send`,
  `::takes back only the queued send that failed when the same text is queued twice`,
  `::leaves no stale entry when an echo takes a failed send's entry before its failure arrives`,
  `ui/src/features/sessions/console/session-console.test.tsx::takes back only the failed send when the same prompt is sent twice at once`,
  `::leaves nothing queued when the echo of the second send comes before the first send's failure`).
- The footer names the keys of the state
  (`ui/src/features/sessions/console/session-console.test.tsx::names the keys of the state in the footer`).
- The blocked banner says where to answer, says to resume an agent that is
  gone, and shows for no other reason
  (`ui/src/features/sessions/session-blocked-banner.test.tsx::says what a permission prompt is waiting for, and where to answer it`,
  `::asks for an answer when the agent asked a question`,
  `::says the agent is gone rather than pointing at the console`,
  `::stays out of the way of every other reason`).
- The typed chords stand aside for a session console's input box, a
  textarea
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
  The drag proof fixes the control's border and padding as well as its
  rectangle: moving from x=60 to x=62 on a 200px track takes 0.3000 to
  0.3100, independent of jsdom's default computed borders.
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
- Editing a test request sends the typed values, trims and drops empty option
  and location lines, and sends `null` for an empty Kind or Workspace;
  invalid JSON shows a field error and disables Test
  (`ui/src/features/permissions/ai-test-panel.test.tsx::sends a typed title, kind, options and locations one per line, edited by hand`,
  `::shows a field error on invalid JSON and disables Test`). These typing
  proofs wait for the dialog's opening focus to reach Examples before editing,
  so its deferred focus cannot interrupt the simulated keystrokes under load.
- The board shows one task area and counts visible tasks; Active hides terminal
  tasks without changing progress
  (`ui/src/features/goals/goal-swimlanes.test.tsx::shows all tasks in one area and counts them`,
  `::hides terminal tasks under Active while preserving progress`).
- Finished, cancelled and failed cards use their status borders
  (`ui/src/features/tasks/task-card.test.tsx::outlines a %s task with its status color`).
- A goal's lane is one line of chevron, title link, status, `done/total` and
  tokens, with no progress bar, and a folded lane shows its summary and no
  cells
  (`ui/src/features/goals/goal-swimlanes.test.tsx::draws a goal as one line: toggle, title link, status, progress and tokens`,
  `::shows the lane summary in place of the cells once folded`).
- The task panel of a stepped task draws one workflow section in place of
  its old Agents fact and step strip: every column on the track, each with
  its agent's skills, model and session under its name, and no description
  (`ui/src/features/tasks/task-panel.test.tsx::shows workflow agents by column`,
  `ui/src/features/tasks/workflow-steps.test.tsx::draws every column on the track with its agent's skills, model and session under it`).
- A pending task's columns are all pending, an in-progress task's own
  column is current with the ones behind it done, and a finished task's
  columns are all done
  (`ui/src/features/tasks/workflow-steps.test.tsx::marks every column pending for a task that has not started, in the pending colour`,
  `::marks the columns behind the current one done, and the rest ahead, in their own colours`,
  `::marks every column done for a finished task, in the completion colour`).
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
- On Linux every scroll area is restyled thin and in theme colours, scoped to a
  `data-platform="linux"` marker that macOS and Android never get
  (`ui/src/lib/platform.test.ts::marks <html> on Linux and leaves macOS and Android unmarked`,
  `::styles scrollbars only under the Linux marker, with both the standard and the WebKit forms`).
- A form dialog's fields and the expanded diff keep right-hand padding for the
  scrollbar, independent of `scrollbar-gutter` support
  (`ui/src/components/form-dialog.test.tsx::keeps room on the right for a Linux scrollbar, whatever the engine supports`,
  `ui/src/features/tasks/task-diff.test.tsx::keeps room on the right of the expanded diff for a Linux scrollbar`).
- The expanded task diff carries a file index above the diff: a filename
  search, every changed file with its own counts, and previous / next buttons
  that scroll to a file's section without changing folding or wrapping; the
  compact and raw views carry no index; an empty diff cannot be expanded,
  and one that becomes empty while expanded stays in the dialog with its
  empty state and no index; nothing is marked as viewed
  (`ui/src/features/tasks/task-diff.test.tsx::filters the expanded diff's file index by name and shows each file's counts`,
  `::scrolls to the next file without changing folding`,
  `::scrolls to the previous file, wrapping to the last from the start`,
  `::offers no file index in the compact view`,
  `::offers no file index in the raw view`,
  `::shows the empty state without a file index when the open expanded diff becomes empty`).
- The Tauri configuration has the restrictive content security policy that
  permits the configurable daemon and Tauri IPC connections without evaluated
  script
  (`ui/src/lib/tauri-config.test.ts::carries a restrictive content security policy`).
- The app-root handler permits only the supported external schemes, opens
  an allowed link directly with no confirmation, leaves an in-app link alone, and does
  neither outside Tauri
  (`ui/src/hooks/use-open-external-links.test.tsx::opens an external link directly, whatever its text says`,
  `::opens a mailto link directly`,
  `::blocks unsafe link schemes before the webview follows them`,
  `::ignores an internal link`, `::leaves a same-origin Tauri fragment link alone`,
  `::leaves a plain browser's link behavior alone`).
- Every pull request reference (task card, task facts, session pull request,
  Forge table and panel) is a `PrLink`: a pull request icon and `#number`, the
  real URL as its `href`, the repository and full URL on hover, an accessible
  name carrying the number, repository and task or title, truncation inside its
  container, and a `PR` fallback when no number can be read from the URL
  (`ui/src/components/pr-link.test.tsx::shows a compact number and keeps the full destination`,
  `::falls back to a compact label when the number is unknown`).
- A failed refresh keeps the cached goals on the board under a retryable
  warning that clears after a successful retry, while a read that never loaded
  shows the load error
  (`ui/src/features/goals/goals-list-page.test.tsx::keeps the cached goals under a retryable warning when a refresh fails, and clears it on success`,
  `::shows a load failure, not a refresh warning, when nothing was ever cached`).
- A failed refresh keeps cached tables, goal task lists, and the task panel
  with its history, empty lists included, under the same retryable warning, and
  a successful retry clears it
  (`ui/src/features/tasks/stale-reads.test.tsx::keeps an empty goal task list under the warning, and clears it on a successful retry`,
  `::keeps the cached task and its empty history under warnings, and a retry clears them`,
  `::keeps the cached task and its empty messages under warnings, and a retry clears them`,
  `::keeps cached table rows under the warning, and shows a load error with none`).
- The goals screen narrows the board by title, names only the search when
  nothing matches, and has no Board/List toggle or repository filter
  (`ui/src/features/goals/goals-list-page.test.tsx::narrows the board by title, and names only the search when nothing matches`,
  `::has no Board/List toggle and no repository filter`); the search matches
  the title alone, never a repository path, and combines with the status
  filters
  (`::matches the title alone, not a repository path`,
  `::narrows by the title search within the selected statuses`).

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

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`,
`ui/dashboard/`, `ui/src/test/flaky.ts`,
`ui/dashboard/src/test/flaky.ts` (the explicit bounded test marker).
