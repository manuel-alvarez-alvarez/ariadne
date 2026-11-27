---
id: desktop-app
status: current
updated: 2026-11-27
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10, 66240a5a]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/features/**/*.test.ts
  - ui/src/routes/**/*.test.tsx
  - ui/src/api/**/*.test.ts
  - ui/src/api/**/*.test.tsx
  - ui/src/events/**/*.test.ts
  - ui/src/events/**/*.test.tsx
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
   In the shell, **Pin inspector** appears from a window width of 72rem.
   Pinning reserves the inspector width beside the overview, without a scrim
   or Tab trap. The inspector remains a named nonmodal region. Nested dialogs
   retain their modal semantics, focus handling and Escape ownership.
   Pinned resizing leaves at least 48rem for the sidebar and overview.
   Unpinning, expansion, or a window narrower than 72rem restores the overlay.
   Collapsing or widening restores the explicit pin choice for this inspector.
   Closing forgets that choice. Mode changes keep the same content mounted,
   without URL or history writes, preserving drafts, transcript position and
   the original focus-return target.
   On Linux (WebKitGTK) every scroll area gets a thin scrollbar in theme
   colours, light and dark, from one rule in `index.css` keyed on a
   `data-platform="linux"` marker; macOS keeps its overlay scrollbars.
3. Screens: the goals board (one list of task rows) has Active,
   All and Finished status segments. A status-menu icon holds a custom
   selection. The rows of one goal are adjacent, and the goal line over them
   carries the goal's tokens in the token column (rule 39). The task
   panel (facts, diff, history), sessions — Ariadne's own,
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
    history keep showing that content when a refresh fails, under
    one `RefreshWarning` ("Could not refresh …", marked as possibly outdated,
    with a Retry for that read alone). A read that never succeeded shows the
    load error instead. The warning leaves with the next successful refresh;
    actions, confirmations and their error feedback are untouched. The goal, task and session panels open on a dense fact list
    above their tabs — `text-xs`, three columns at `sm` and four at `lg`, no
    card frame — rather than the framed grid further down an entity's own
    screen; a stepped task's own workflow section is rule 40's.
21. Every session is shown in its console, which draws the turns of the
    daemon's trajectory (040): rules 89 to 109 say what a turn shows. The
    session view mounts it in the session panel, a task, a goal and a pull
    request, with no switch and no URL parameter. The UI reads a session through
    the trajectory routes alone
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::shows the turn console in the session view, with no parameter`,
    `ui/src/features/sessions/session-mounts.test.tsx::shows the turn console in the session panel`,
    `::shows the turn console in a task`,
    `::shows the turn console in a goal`,
    `::shows the turn console in a pull request`).
    The daemon folds the events, and the console folds none. A person's prompt
    and a developer prompt (`source: "daemon"`) are Markdown, drawn by
    `components/markdown.tsx` as a message is: `h1` to `h3` in a visible scale
    above body text, lists with their nesting, task-list checkboxes, `del`,
    `kbd` and inline code styled, a single newline in a prompt still a line
    break, and a fenced block a `CodeBlock`
    (`ui/src/features/sessions/console/blocks.test.tsx::renders a prompt as Markdown: headings, task lists, a fenced block, and a single newline still breaks`,
    `::renders developer prompts as full Markdown without a fold`,
    `::draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text`,
    `::shows the time of a prompt and marks a queued one`).
22. An input box posts a prompt as `{"text"}` to
    `POST /v1/sessions/{id}/console/input`: Enter sends it, and Shift+Enter
    adds a line. A failed post is kept, rule 22d. The box is disabled while
    the session is not live. A running turn shows a Stop control that posts
    `POST /v1/sessions/{id}/console/cancel` with no body, and Escape inside
    the console cancels it too. A pending permission is answered as rule 93
    says
    (`ui/src/features/sessions/console/composer.test.tsx::disables the input and Send while the session is not live`,
    `ui/src/features/sessions/trajectory/trajectory-console.test.tsx::cancels a running turn with Stop and with Escape`,
    `ui/src/features/sessions/console/composer.test.tsx::gives the input the keyboard when asked to on arrival`).
22b. The composer (`console/composer.tsx`) starts at one line and grows with
    newlines and pasted text to six lines, then scrolls inside. Send is an icon
    button with a tooltip at its right edge, aligned with its last line and never
    wrapping under it, disabled while the draft is blank or the session is not
    live, and Stop is one beside it while a turn runs. Enter sends and Shift+Enter adds a
    line; an Enter that ends an input-method composition does neither, and a
    pasted text keeps its lines.
22c. Text typed in the composer always posts to the input route (rule 22).
    The composer answers no question.
22d. A message the daemon refuses leaves the queue and stays in the
    console with the error, a Retry and an Edit. Retry sends the same
    text again and leaves the draft as it is. Edit takes the text back into the composer in front
    of any newer draft, and gives the composer the keyboard. Refused messages are
    retained with the console's state (rule 99g), so Expand, Collapse and a
    change of tab keep them, and a send that fails after the pane was
    replaced shows in the pane now mounted.
22e. A plus menu beside the composer searches, by name and description, the
    agent's commands (rule 23c) and these actions: Switch agent/model, where
    the session's goal is not cancelled; Resume, only while the session is
    not live; and `Copy loaded transcript (main)`, which copies the prompt and
    the answer of each turn the console has loaded, and nothing older. Switch and Resume are the session actions of
    the session panel (`session-actions.tsx`), with the same dialog, toasts
    and navigation to the successor or the revived session. A command picked
    from the menu goes into the composer for editing in front of any draft,
    with the caret after it, and is never sent. After a pick the composer
    has the keyboard. Every entry is a button, so a click, a tap and the
    keyboard reach it: the search field takes the keyboard on opening, Down
    walks the entries, Up returns to the field and Enter in the field picks
    the first match. A long command name or copy label is cut with an
    ellipsis, whole on hover, and the list never scrolls sideways. The menu has
    no global fold action.
22f. A question the agent asks with its own tool, an ACP form elicitation
    (021), is a form card (`console/form-card.tsx`). The console reads
    `GET /v1/sessions/{id}/questions` and draws each open question of a turn's
    `attention` directly above the composer, as rule 94 says.
    Each question occupies one step, in the console body font and size.
    The header shows its title or "Question", clock time, and position for multiple questions.
    Question tabs mark answered questions and allow navigation.
    Markdown draws the message and the field description for one question, and the field description for several.
    Large numbered choice rows show titles and descriptions.
    A case-insensitive "(Recommended)" title suffix becomes a Recommended badge.
    A single pick advances, or sends immediately on the last step.
    Multi-select check rows use Next, or Send on the last step.
    An Other companion is a string field without choices, directly after its choice field,
    named `<choice key>_custom`. Claude emits `question_N` and `question_N_custom`;
    Codex and OpenCode emit no companion. A choice and its companion form one question.
    The companion appears as "Type your own answer…". Enter confirms typed text.
    A free-text question has one input. Other fields, including numbers, each occupy one step.
    Picks remain in `content[key]`; typed text remains in `content[key_custom]`.
    A custom single-select answer without a pick sends only `content[key_custom]`.
    Skip omits the question and its companion. Dismiss posts `{"action":"decline"}`.
    Accept posts `{"action":"accept","content":{...}}` to
    `POST /v1/sessions/{id}/questions/{question_id}/answer`.
    The card takes focus when opened. Scoped keys 1–9 select rows, arrows move focus,
    Enter confirms, and Back returns. Composer keys remain with the composer.
    A failed post displays its error and allows another attempt.
    A closed card folds to one line per question: "<field title, else message> → <answer>".
    Custom text merges into that question, without raw companion keys.
    Dismissed, withdrawn, cancelled, and session-closed cards show one status line.
    Clicking the summary opens the full card read-only, with picks marked.
    An attention-strip form with one single-select question, including its companion,
    answers in place with one click and displays the Recommended badge.
    Other forms open the console. A failed read of the list shows its
    error with Retry, which reads the list again. A read of the list that lands
    after the console moved to another session draws nothing. A session
    that is not live takes no answer. While a question is open the console's
    status row says `waiting for you`, and the session activity feed labels
    its `question` event `Waiting for you`
    (`ui/src/features/sessions/console/form-card.test.tsx::draws one question at a time with large choice rows`,
    `::sends the answers as accept with the content of the form`,
    `::sends decline when the user dismisses the question`,
    `::shows a failed answer on the card and lets the user try again`,
    `::draws no card of the session it left when its question read lands late`,
    `::says the session waits for the user while a question is open`,
    `ui/src/features/sessions/console/form-questions.test.ts::reads each field of the schema with its choices as the agent wrote them`).
23. The console uses the app's own tokens, in both themes. An Expand control
    opens it in a near-fullscreen modal, and its Collapse control restores
    it to the panel; each move mounts a new pane, which opens a new stream
    and reads the console's retained state (rule 99g).
    Escape inside the console belongs to the console in the panel and in
    the modal alike; Escape outside the console closes the panel or the
    modal. ⌘Escape leaves the console without cancelling or closing
    anything itself: it moves focus to the panel's Close button, or to the
    modal's Collapse button, so the plain Escape that follows closes what
    is now focused. Ctrl-O has no handler in the browser and desktop console
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::moves the keyboard to the pane's Close button on ⌘Escape, and closes nothing`,
    `ui/src/components/panel-sheet.test.tsx::leaves Escape on the board alone and returns focus when the pane closes`).
23a2. Code is a `CodeBlock` (`components/code-block.tsx`), in the Markdown
    renderer's fenced blocks and in the console's tool output, commands and
    diffs. `lib/highlight.ts` resolves a fence's name to a language of
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
    prompt is
    (`ui/src/features/sessions/console/composer.test.tsx::completes a suggestion by keyboard or click and keeps the text after the command`,
    `::completes a suggestion on a click, keeping the composer's focus`,
    `::shows at most eight suggestions, and says none matches`,
    `::closes the suggestions on Escape without cancelling, and opens them again on the next edit`).
25. A session's view has two tabs over one space: the console, open by
    default, and the agent activity feed. The tab is in the URL (`?tab=`);
    the console's tab is `terminal` on the wire, so an older link still
    opens it. A tab value that is not one of the two opens the console.
    Leaving the console's tab closes its stream, and coming back opens a
    new one
    (`ui/src/features/sessions/console/composer.test.tsx::holds one open stream for the console shown, and none once it is hidden`).
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
    the web, Read the task, Complete the step, Pipe a script to the
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
    the example's locations, options and workspace`, `::sends manually edited
    title, kind, options and locations one per line`,
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
    shown task rows, the tasks of folded goals included. The board is one
    list, and each shown task is one full-width row. Active hides finished
    and cancelled tasks; All, Finished and custom filters show every task in
    each matching goal. Progress counts use all tasks.
    A task row shows on one line the facts of the task card: the title, the
    session attention badge, the state icon, the host label, the step track,
    the token figure, the PR link and the last activity. The board is one
    grid and each row takes its columns, so the columns align from row to
    row. The board has no line of column names. The token column is as wide
    as the largest token figure, so no board width cuts a figure. The row has
    no border and no fill. Pointer hover and keyboard focus give it a soft
    tint. It is one link to `?task=` with one tab stop, and the PR link is a
    real anchor outside it with no tab stop.
    The board shows a state as an icon, not a pill. A task row shows one
    coloured icon for every status: pending a clock, ready a play circle, in
    progress a spinner circle, finished a check circle, failed a cross circle,
    cancelled a ban sign, and a stalled task a warning triangle in place of its
    status. A goal line shows its status the same way: planning a compass,
    active a spinner circle, completed a check circle, cancelled a ban sign.
    Each state has its own shape, so colour is never the only carrier. The icon
    takes its colour from the `status-*` tokens of `ui/src/index.css`, and the
    muted foreground for cancelled; no token is added. The icon's accessible
    name is the status word ("Pending" for a waiting row, "Stalled" for a
    stalled one), and its tooltip leads with it and adds the detail, such as
    "Waits for N tasks to merge". A row whose execution owner is unconfigured shows a
    warning icon named "Unconfigured worker", with the words in its tooltip,
    in place of the text. The panes keep the state pill with its word
    (`ui/src/features/goals/goal-board.test.tsx::shows each task state as its own coloured icon with the word as its name and tooltip`,
    `::shows a stalled task as a warning icon`,
    `::names a waiting task's icon by its state word and keeps the dependencies in the tooltip`,
    `::shows an unresolved execution owner on a row as a warning icon`,
    `::shows each goal state as its own coloured icon`,
    `ui/src/features/tasks/task-panel.test.tsx::shows the task's state in the pane as a pill with its word`,
    `ui/src/features/goals/goal-panel.test.tsx::shows the goal's state in the pane as a pill with its word`).
    Each row has a rail segment at its left edge. The segment takes the
    colour the card's border has, in the card's order of precedence, and
    the border colour when the card has none. The rows of one goal are
    adjacent, in a named group (`<goal>: Tasks`), so their segments join into
    one rail. A gap separates one group from the next. A task with no goal is
    a group of one row, with its own segment.
    From 33.9rem wide up, the board never scrolls sideways. Below 33.9rem it
    scrolls sideways and cuts no column. It is a CSS container, so its own width
    decides the columns, and a pinned inspector narrows it like a narrow
    window. Below 60rem the step names hide, below 54rem the last activity,
    below 48rem the host, and below 42rem the PR link. A hidden column takes
    no width, and a goal line's host label hides with the host column. The
    title, the state, the step dots and the tokens stay at every board width
    the app draws, and a title that does not fit ends in an ellipsis. The
    narrowest such board is 33.9rem: the window is at least 900px wide, and a
    pinned inspector needs a 72rem window and leaves 48rem beside it. The
    state, the dots and the tokens never shrink below their content: the
    state column takes its badges' width, the token column its widest
    figure, and the step column the widest dots of any workflow on the
    board, folded where that workflow folds, above the column's own
    maximum where needed. Only the current step's name and
    the title give up width.
    The groups sort in three bands: the groups that need the user, then the
    live groups, then the finished or cancelled groups. A goal needs the user
    when its orchestrator or one of its agents has an attention flag, or one
    of its tasks failed or stalled. In a band, the group with the newest
    `updated_at` is first. A task with no goal sorts by its own state and its
    own `updated_at`, by the same rule.
    Each goal has one goal line above its rows, visible at rest, with no
    hover and no focus. The line has no background, no border and no dashed
    line, and the board draws no frame around the rows of a goal. The goal
    line is: a fold chevron, the title as a link to `?goal=`, the host label
    by rule 74, the status, `done/total`, the orchestrator attention badge,
    and the goal's token figure in the token column, directly above the
    token figures of its rows. The keyboard reaches the chevron and the
    link. The title's tooltip gives the full title, each repository as
    `path [base_branch]`, the created time, and the progress as done against
    failed and stalled. The line has no progress bar and no repository
    summary.
    A goal with no shown task is its goal line, then one muted line with the
    state text and no border. The state text is "Tasks are not loaded" (rule
    58), "No tasks yet — the orchestrator is still working" for a planning
    goal, "No tasks", or "No active tasks" where Active hides every task of
    the goal. Unfolding to no shown task leaves the keyboard on the chevron.
    A folded goal is its goal line with the one-line summary text in place
    of `done/total`, no border and no task row.
    Finished and cancelled goals start folded, and the fold the user picks
    for a goal is kept in `localStorage`.
    Attention blocks: an unfolded goal shows, below each task row, one block
    for each attention item that names the task, or one of its sessions
    through the shared sessions list. An item that names many tasks shows one
    block below each. Directly below the goal line, it shows one block for each
    item that names the goal, or a session of the goal with no task. A task
    with no goal shows its blocks below its row. A folded goal shows no block,
    and its goal line keeps the attention badge. A block is the drawer row
    (`AttentionBlock`, shared with the popover and the drawer): the reason,
    the summary, the required action, the age, the affected links, the target
    link and Dismiss. A form of one single-select question (rule 59) shows its
    message and one button for each choice; every other form links to the
    console. The blocks come only from `GET /v1/attention`, also during an
    incomplete read, and follow its query with no new polling and no new
    stream. The blocks keep the columns and width rules above, and each link
    and button of a block takes a tab stop.
    A task card is three contained rows, for every status, in the goal
    panel. Finished cards have green borders, cancelled cards gray borders,
    and failed cards red borders. Its target minimum width is 18rem (about
    288px), and a card shrinks inside a narrower panel. The card is a CSS `@container`, so its own width decides what is
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
    the goal line's own `TokenFigure` at every width; its hover names
    Input and Output and lists one row per workflow step, and it is not a tab
    stop) at the left, then, when the task has a `pr_url`, a compact PR link
    (icon and `#number`; a real anchor outside the card's link with no tab stop,
    and the hidden hints still name the request), then last activity at the
    right end. A review or babysit task shows its goal's request through its `pr_url` (012 rule 3). The PR link and the time never overlap; the
    token figure is cut first. The time shortens ("3 minutes ago" to "3m") and truncates as the card narrows. Last
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
40a. A remote task panel uses its saved source title and workflow when no
    worker goal exists. A host that cannot be reached marks progress as
    potentially stale (`ui/src/features/tasks/task-panel.test.tsx::shows the saved source and workflow when the worker has no goal`,
    `ui/src/features/tasks/task-card.test.tsx::shows the colored remote host and its unreachable state`,
    `ui/src/features/goals/goal-board.test.tsx::shows the execution host of a projected task in its row`,
    `ui/src/features/command-palette/hosts.test.tsx::shows a source projection and worker response as one task`).
    Source navigation and worker labels match daemon identities to configured
    hosts; an unmatched worker is labelled unconfigured. Cancellation and
    retry feedback names the returned status; a failed retry response says the request
    awaits worker acknowledgment rather than claiming recovery
    (`ui/src/features/tasks/task-card.test.tsx::labels a projected task by worker identity and keeps the source route`,
    `::labels an unresolved execution owner as unconfigured`,
    `ui/src/features/tasks/task-action-feedback.test.tsx::reports cancellation response %s`,
    `::reports retry response %s`,
    `::reports a retry request awaiting worker acknowledgment`). Worker
    drafts use their saved workflow without reading the goal. Their disabled
    repository picker shows the assigned repository from the worker catalog.
    Remote cancellation waits for the daemon
    (`ui/src/features/tasks/hosts.test.tsx::opens the source goal from a worker task through the matching daemon identity`,
    `::edits a worker draft using its saved workflow without a goal`,
    `ui/src/features/sessions/hosts.test.tsx::links a worker session to its source goal without reading a worker goal`,
    `ui/src/features/tasks/task-panel.test.tsx::keeps remote task progress while cancellation is pending`,
    `ui/src/features/sessions/hosts.test.tsx::names a worker session by the task's saved workflow`,
    `ui/src/events/dispatch.test.ts::refreshes the source goal usage when a remote task changes`).
41. The task form shows one row per column of the goal's workflow, prefilled
    with the column's skills, with the column's preferred rank beside the pin
    picker, and sends `agents`, seat `agent` and the column's `step` each, on
    create and on edit, where an edit replaces the whole list; a goal with no
    steps gets no staffing row at all.
The goal and task forms start with title and Markdown description. They place host,
repositories, workflow, agent settings, and dependencies in that order where applicable.
The repository picker searches paths. Goals retain pick order; a new task picks one
repository, and an edited task shows its assigned repository without allowing changes.
Required model pickers name the agent that needs a model and prevent submission until
all models are selected (`ui/src/features/tasks/task-form-dialog.test.tsx::shows the sole repository and names agents that need models`,
`::selects one repository and closes the search`).

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
    Markdown, through the same renderer as descriptions, so a
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

49. Goal title search stays in the URL as `q` beside status.

50. The request panel shows a **Review** button group on an open request (029 rule 8).
    It offers **Start review** when no review runs, and **Stop review** with **New turn** while one runs.
    The **Babysit** group is the rule of 032 rules 16 and 17.
    The group label tells the two **New turn** buttons apart.
    The Review **New turn** button calls the round route and shows the daemon's refusal text.
    Proof: `ui/src/features/pull-requests/pull-request-panel.test.tsx::asks for another review round and shows the daemon refusal`.
    Proof: `pull-request-panel.test.tsx::groups the review and babysit buttons, each group by what runs`, `…::shows Start review and Start babysit in their groups when nothing runs`, `…::shows only New turn in the babysit group of a task-kept request` and `…::shows no group on a request that is not open`.

51. The app reads every host of the settings daemon's `GET /v1/hosts` (037).
    `local` is the settings daemon, at the settings base URL. A named host is
    reached at its endpoint only when the endpoint is `http://` or `https://`
    and its `status` is `ready`; any other host is listed as unreachable with
    its `error`. `host_updated` patches its row. The picker, the dashboards
    host select and the logs host select show only while the daemon lists a
    named host; a named host that is down counts as listed.
52. The Hosts screen, at `#/hosts` and titled "Hosts", lists each host's `name`,
    `kind`, `status`, `endpoint` and `error`, the facts `ariadne host ls`
    prints, live through `host_updated`. Hosts come from setup or the settings daemon's `config.toml`. It stays with `local` alone.
    Each named host's row has a reconnect button, named "Reconnect" and the
    host name, in each status. A click sends `POST /v1/hosts/{name}/reconnect`
    to the settings daemon and the button is disabled while that request
    runs. A request that fails shows its error in a toast. The row shows each
    new status through `host_updated`. `local` has no button. The Status
    column shows the app's reach of the host, in the words of rule 55, and
    the Error column its reason. A Daemon column shows the version and the
    uptime of each reachable host, from the last heartbeat of the app's
    stream to it, and "-" before one arrives or for a host the app cannot
    reach (rule 78).
    The screen reads the hosts of `local` only, so the header host picker
    does not change it. The settings dialog has no Hosts section.
    Its page header has one "Add machine" action, which opens the dialog of
    rule 52a. Each row has an Edit action. For a named host
    the dialog has endpoint, remote and colour, with the name read-only; for
    `local` it has colour alone, with no name field. Its submit sends `PATCH /v1/hosts/{name}` with
    the changed fields only, and an emptied remote is `null`. The colour field
    offers "Default" and the eight colours of `HOST_COLORS` as swatches, and
    "Default" sends `null`, on add as on edit. Each named host's row has a Remove action, which
    asks for confirmation, says that the daemon on the host keeps running, and
    sends `DELETE /v1/hosts/{name}`; `local` has none. A dialog shows a 400 or
    409 at its field when the message names one, and otherwise in the dialog;
    a 409 that names no field shows in the dialog.
    A failed removal shows its error in a toast. A submit button is disabled
    while its request runs. The table follows `host_updated` and
    `host_deleted` with no reload.
    (`ui/src/features/hosts/host-edit.test.tsx::adds a host with name, address, colour and an Advanced remote socket, and shows its row through host_updated`,
    `::offers Default and the eight colours as swatches`,
    `::sends only the changed fields of a named host, an emptied remote as null and Default as null`,
    `::edits the colour of local alone`,
    `::sends color null when a new host keeps the Default colour`,
    `::shows a refusal at the field it names, and any other in the dialog`,
    `::disables the submit button while its request runs`,
    `::removes a named host after a confirmation that says the far daemon keeps running`,
    `::shows the error of a failed removal in a toast`).
    (`ui/src/features/hosts/hosts-page.test.tsx::titles the screen Hosts and opens the machine dialog from its Add machine action`,
    `::shows the hosts of local whatever the header host picker selects`,
    `ui/src/components/settings-dialog.test.tsx::has no Hosts section`).
    The sidebar lists Hosts after Agents, and the chord `g h` opens it
    (`ui/src/components/app-shell.test.tsx`,
    `ui/src/hooks/use-global-shortcuts.test.tsx::opens the Hosts screen on G then H`,
    `ui/src/components/keyboard-shortcuts-dialog.test.tsx::lists every chord, screen by screen`).
52a. "Add machine" is one step-by-step form dialog (`FormDialog`, `Field`).
    Its first step asks for the method: "Install Ariadne on a machine through
    SSH" or "Connect to a daemon that already runs", each with one line that
    says when to use it. Every step has Back, Cancel and its submit. A
    disabled submit shows its reason above the buttons. The dialog says "this
    machine" and "the new machine", and never "worker", "source daemon
    machine" or "settings daemon". An "Advanced" section holds each input that
    a default replaces.
    - The connect method asks for the name, the address and the colour, with
      the remote socket under "Advanced". Its submit sends `POST /v1/hosts` to
      `local`, as rule 52 states.
    - The install method has the steps Connect, Select, Review and Install,
      and a step bar shows the position. Connect asks for the name and the
      SSH address, and says that Ariadne uses the SSH keys of this machine and
      never asks for a password. Next sends
      `POST /v1/hosts/{name}/setup/inspect` (spec 037, rule 75).
    - Select shows the Ariadne programs that inspection reports as one line:
      the method and the version. "Advanced" holds the remote socket, the two
      program paths and the release inputs. The request has a `source` only
      when the user fills them. Select lists each agent and GitHub CLI with a
      checkbox and its group: Ariadne installs it, it is already on the new
      machine, or the user installs it by hand with the command shown. The
      agents that run on this machine, GitHub CLI and every registered
      repository start selected. Each repository has a destination that starts
      as `~/workspace/<name>`. Each sign-in that the selection needs offers
      "Sign in on the new machine", the default with its command, and "Copy
      from this machine", disabled where inspection does not permit a copy.
      Next sends `POST /v1/hosts/{name}/setup/review`.
    - Review shows the plan under "Ariadne installs", "Already on the
      machine" and "You install by hand", and names each sign-in that setup
      copies. Back keeps every input. Select reviews again on each Next, so
      Install sends the request that was reviewed: `POST /v1/hosts/{name}/setup`.
    - Install is `SetupProgress` (`features/hosts/setup-progress.tsx`), which
      takes a setup name. It polls `GET /v1/hosts/{name}/setup` while the setup
      is `pending` or `running`, and shows each step by its title. A step that
      needs manual work shows its reason, its command with a copy control, and
      "Check again", which sends `POST /v1/hosts/{name}/setup/resume`. A
      stopped setup also shows "Skip" beside the manual instructions of each
      failed step that the daemon lets the user skip (037, rule 49). Skip sends
      `POST /v1/hosts/{name}/setup/steps/{step}/skip`. A skipped step shows
      "Skipped" and its manual instructions, with no "Check again" and no
      "Skip". When no step that is not skipped needs manual work, "Continue
      setup" (or "Continue update") resumes, and setup goes on past the skipped
      steps. The dialog closes during Install with no question, and the setup
      continues in the daemon.
    (`ui/src/features/hosts/add-machine.test.tsx::replaces Add host and Set up worker with one Add machine action that asks for the method`,
    `::asks Connect for the name and the SSH address alone, says why Next waits, and inspects`,
    `::shows the programs as one line and selects available agents, GitHub CLI and every repository`,
    `::asks one sign-in choice per needed sign-in, and copies only where inspection permits it`,
    `::sends a source only when the Advanced inputs name the programs, and says why Next waits`,
    `::reviews the plan in three groups, keeps the inputs on Back, and reviews a changed selection again`,
    `::closes during the installation with no question, and the setup continues in the daemon`,
    `ui/src/features/hosts/host-edit.test.tsx::shows a refusal at the field it names, and any other in the dialog`,
    `ui/src/features/hosts/setup-progress.test.tsx::shows each step by its title, and a manual action with its reason, its command and a copy control`,
    `::resumes the setup from Check again and shows the progress it answers with`,
    `::offers Skip beside the manual work of a failed tool, shows it skipped, and continues the setup past it`,
    `::offers no Skip for a failed step of Ariadne itself or of Git`).
52b. Each host row can carry the actions of its machine, and no action asks
    for a typed name. The table reads `GET /v1/host-setups` every second while
    an operation is `pending` or `running`, so it stays current with no manual
    reload. The page header has no "Update worker" button.
    - Each saved setup or update that is not complete has its own row, with
      its name and its state, also where a host of that name has a row. The
      "Continue setup" of a setup opens the Install step of "Add machine" for
      it, with live progress and the manual actions. The row of an update says
      "Continue update" instead, and opens the update dialog on its progress. "Discard setup" appears for a setup
      whose state is `incomplete` or `failed`, and never for an update, which
      the daemon keeps (037, rule 80). It asks first, says that files on the
      remote machine stay, and sends `DELETE /v1/hosts/{name}/setup`. A setup
      that starts from "Add machine" refreshes the list, so its row appears
      and the poll starts.
    - A host with a complete saved setup has "Update". Its dialog is the form
      dialog of "Add machine", and its title names the machine. It has no name
      and no SSH input. It has two independent selections. "Ariadne programs"
      is selected and updates to the version of this machine (`source` is
      `{ "kind": "daemon" }`); "Advanced" holds the two program paths and the
      release inputs. "Agents and tools" lists each agent by name and GitHub
      CLI, and the saved selection of the setup starts selected. A request
      with no program selection has no `source`. The dialog says that the
      update waits for running tasks, then shows live progress with the step
      titles and the manual actions through `SetupProgress`. A failed update
      goes back to the selections with Back, and Update sends the request
      again. The words "worker", "source daemon machine" and "settings
      daemon" do not appear on the screen.
    (`ui/src/features/hosts/host-setups.test.tsx::shows a row for each saved setup that is not complete, with its state and Continue and Discard actions`,
    `::offers no Discard setup while the setup runs`,
    `::opens the Install step of the setup from Continue setup`,
    `::discards a stopped setup after a confirmation that says the remote files stay`,
    `::updates a machine with no name or SSH input, the programs of this machine and the saved tools selected`,
    `::sends tools alone when the programs are not selected, and says why it waits when nothing is`,
    `::lets a failed update change its selections and try again`,
    `::reads the saved setups again every second while an operation runs, and stops when it ends`,
    `ui/src/features/hosts/add-machine.test.tsx::shows the row of a setup that starts while the saved setups list was empty`).
53. `api(host)` is one host's typed client, and `eventStreamUrl(host, filters)`
    builds its stream URL. The app opens one domain stream per reachable host,
    and each event patches and invalidates the keys of the host it came from.
    A gap in one host's stream invalidates that host's keys alone. A host
    that came up or moved since the last hosts list is read again whole, so a
    `host_updated` missed while `local`'s stream was down leaves nothing
    stale. `session_created` and `session_updated` patch the row into every
    cached sessions list it belongs in, newest activity first, and refetch
    none of them. A list whose read is still under way is refetched instead.
    The
    connection banner and status stay about `local`; a named host's state is
    its row in the host picker.
54. Every query key starts with its host: `[host, entity, "list" | "detail",
    ...]`, and `qk.<entity>.lists(host)` invalidates one host's lists.
55. The header has a host picker, shown only with a named host: All hosts,
    `local`, and each named host with its status. A host the app cannot reach
    reads "not reachable" with its reason, whatever its own status says, so a
    host with a socket path never reads as ready. While the app's stream to a
    reachable host is not connected, its row also says "stream" and the
    stream's state, and while its sync is pending or failed, "sync" and the
    sync line of rule 65, with the error of a failed one. With All hosts
    picked, the trigger shows a mark while a listed named host is not
    reachable or has a failed sync. The trigger shows the status
    of a picked named host. The pick is `hostFilter` in the settings store and
    persists. Each named host's row has the same reconnect button as rule 52,
    and a click on it changes no pick. A pick the daemon no longer lists
    reads as All hosts in every host control. All hosts reads every reachable host; `useHostFilter()`
    returns the selected names.
56. The panel params `?goal=`, `?task=`, `?session=` and `?pr=` have a
    `?host=` beside them for a named host. A link without `?host=` resolves to
    `local`.
57. `useHostLists` runs one list query per selected host. It returns the rows
    tagged with `host`, and one error per host that failed.
58. The goals board merges the goals of the selected hosts, and each goal
    names its host by rule 74. A goal of a host whose tasks did not load, or
    have not loaded yet, says "Tasks are not loaded" and never "No tasks". The board also follows rule 80. A goal's panel
    opens on its host, with its tasks and orchestrator sessions read there. A
    goal lives on one host. The goal dialog shows a host select once there is
    more than one listed host. The select defaults to the picked host when the
    app can reach it, else to `local`. It lists each listed host; a host the app
    cannot reach is a disabled option with its reason. Its repositories, workflows and model pin picker read
    that host, and a change of host reloads them. The pin picker takes a host.
59. The attention strip, its count in the shell and in the window title, and its
    toasts read every reachable host for each host pick, and merge their items.
    Each row names its host whenever a named host is listed, under any pick;
    rule 74's no-host row under one pick does not apply to it. A toast names its item's
    host, `local` included, whenever a named host is listed. A form question
    (rule 22f) of one single-select field is answered in the row too, with one
    button per choice that posts `accept` to that host's
    `POST /v1/sessions/{id}/questions/{question_id}/answer`; any other form is
    answered in the console its row opens. A
    row of a session that waits for a permission (031, rule 11a) shows the
    oldest pending permission of that session, read from
    `GET /v1/sessions/{id}/trajectory` on the host of the item: the tool, its
    subject, the question and one button for each option, and the subagent
    that asked. A click posts the option id as `text` to that host's
    `POST /v1/sessions/{id}/console/input`. Every button is disabled while the
    post runs, in every view of that permission, and a failed post shows its error and permits a new attempt. A permission whose answer the daemon accepted stays disabled in every view until the trajectory shows another pending permission, even when the list refresh fails
    (`ui/src/features/goals/attention-permissions.test.tsx::disables every option while the post runs`,
    `ui/src/features/goals/attention-permissions.test.tsx::disables the buttons of every view of the permission while one view posts`,
    `ui/src/features/goals/attention-permissions.test.tsx::shows a failed post and permits a new attempt`,
    `ui/src/features/goals/attention-permissions.test.tsx::keeps an answered permission disabled when the attention list refresh fails`). A session with no
    pending permission shows no button and keeps the console link. The
    trajectory is read again after each change of the host's attention list.
    The board, the popover and the drawer show the same buttons
    (`ui/src/features/goals/attention-permissions.test.tsx::draws the same buttons in the popover and in the drawer`). An attention
    error of one host follows rule 75: it names that host and has Retry; the items of the other
    hosts stay and their toasts continue. The first answer of a host raises no
    toast, also for a host that answers late, becomes listed, or comes back: a
    returning host's cache is not an answer, so what it raised while away is
    not announced.
60. The task panel (facts, steps, diff, sessions, history) reads and
    writes through `api(host)` for the `?host=` of its route. Retry, cancel and
    update post to that host. The task dialog writes to the host of its goal,
    and its model pin pickers read that host. A task card of a named host
    links to its panel on that host.
61. The sessions screen merges the sessions of the selected hosts with
    `useHostLists`. Each row names its host by rule 74. The status and seat
    chips apply to every selected host. The goal chip and the task chip each
    keep the host of their id, in `?goal_host=` and `?task_host=` and in the
    remembered pick, and `local` when it is absent. A chip applies only while
    its host is selected; while it does not, the screen shows no chip and does
    not narrow the list, and the pick stays for when its host is selected
    again. Clear filters removes the goal chip and the task chip too. The
    screen also follows rule 80.
    The Agent filter lists the agents of each selected host.
62. The session panel and the console open their streams on the session's
    host. `console-view-state.ts` keys its state by host and session. `local`
    adds its daemon address to the key, so each daemon keeps its own state; a
    named host does not, so its draft, scroll position, view, folds and failed
    sends stay when its address changes. While the host is not reachable, the
    console opens no stream and its status line gives the reason of the host.
    Console input, cancel and answers post to the session's host. A
    switch of a session's model lists the models of that host. A goal panel's
    Sessions tab opens sessions of a named host the same way.
63. Every attention target opens its item on its host. An item about the agents
    of a named host opens `/agents?host=<name>`; when that host is outside the
    pick, following the link also sets the pick to All hosts.
64. The command palette searches the goal, task and session lists of every
    reachable host for each host pick, and the skill list of `local`. Each row
    carries its host, names it whenever a named host is listed, under any pick
    (not rule 74's no-host row under one pick), and opens on it. The `ariadne attach <id>`
    action copies `--host NAME` when the host is not `local`. "New task" is
    offered for the open goal, read on the host of its `?host=` under any pick.
65. The skills, workflows, learned permissions, AI permission and repositories
    screens read and write `local` only, and the host picker does not change
    them. While a named host is listed, each says that it shows and writes the
    settings of `local`, and sets the lines under a heading, "Sync to other
    hosts": one per named host from `HostDto.sync`: in sync, pending, or failed
    with the error. A pending sync of a host the app cannot reach reads
    "waiting for host", since it waits for the host and not for the daemon. The Learned tab's line also says how many local learned
    permissions the host skipped, from `sync.skipped_permissions`. With only
    `local` listed, no screen shows the note, the heading or the lines
    (`ui/src/components/host-sync.test.tsx::says it shows and writes local, and heads the sync lines of the named hosts`,
    `::shows no note, heading or sync line with only local listed`,
    `ui/src/features/permissions/permissions-hosts.test.tsx::shows a sync line per host: in sync, pending, or failed with its error`,
    `::counts the learned rows a host skipped on the Learned tab only`).
66. The AI permission model's `device` is not synced. The AI tab shows a card
    per host, read from that host's `GET /v1/permissions/ai`, with its install
    state, hardware, Refresh and Device. A host the app cannot reach shows its
    reason in place of the card, with no skeleton. A device change sends
    `PUT /v1/permissions/ai` with `device` alone to that host. The switch,
    thresholds, flavour and test panel are `local`'s card only.
67. The repositories table has a Hosts column. For each named host it says
    whether the host has a checkout of the repository or lacks one, from
    `sync.missing_repositories`. Until the host's first sync completes
    (`sync.state` is `pending`) it says pending, since the list is empty then
    and says nothing about a checkout. It says the same "waiting for host" as
    rule 65 for a host the app cannot reach. A repository with no forge is
    always in the host's `missing_repositories`, since a checkout is matched
    by forge; the table says "no forge to match by" for it, not "no
    checkout". With only `local` listed the table has no
    Hosts column. The forge tunnel switch is `local`'s
    (`ui/src/features/repositories/repositories-page.test.tsx::says which hosts hold a checkout of each repository and which lack one`,
    `::shows a host as pending, not as holding a checkout, until its first sync completes`,
    `::reads a pending sync of a host the app cannot reach as waiting for host`).
68. The agents screen reads the agents and models of the selected hosts. One tab
    holds one agent id. Its panel has a chip per host that has the agent where
    rule 74 shows the host on rows. A flag edit, a model switch and a rank apply to
    the host of the chip picked, and Refresh reprobes every selected host. Each host
    whose agents or models failed to load follows rule 75.
    The chip belongs to the screen: it stays picked when the tab changes to an
    agent on that host, and `/agents?host=<name>` opens it with that host's chip
    picked. A `?host=` beside a panel param is the panel's host and never moves the
    chip. While the picked host's models load, the model table shows a loading
    state and says "No models" only once the host answered none. If they failed,
    the table shows the error with the host name and Retry. A tab count and the
    count of models turned off, in the Refresh tooltip, are the picked host's own
    and never a sum or another host's value. With more than one host selected, the
    flags dialog title and the toasts of a flag, model and rank change name the host.
    Tests in `ui/src/features/agents/agents-hosts.test.tsx`:
    “shows a loading table, not 'No models', while the picked host's models load”,
    “says 'No models' once the picked host answered none”,
    “shows the error of the picked host's models in the table, with its name and Retry”,
    “counts the models of the picked chip's host, and the models turned off there”,
    “keeps the picked chip when the agent tab changes”,
    “names the host in the flags dialog and in the toasts of a model change”,
    “names the host in the toast of a saved flag list”,
    “names the host in the toast of a refused rank change” and
    “opens with the chip of ?host= picked” and
    “keeps the chip when a panel opens over the screen with its own ?host=”.
69. The Forge tabs merge the pull requests and issues of the selected hosts,
    most recently updated first. A repository filter names one forge
    repository. Each host reads it by its own id for that forge repository; a
    host without it is skipped. Paging, the text filter, the Whose filter and
    the repository filter give each request one time. A host that fails keeps
    its error apart, and the rows of the other hosts stay.
    - A request that many hosts list is one row, matched by its forge
      repository and its number. The row shows the newest forge facts among
      those hosts. Under rule 74 it names the host of each Ariadne review and
      each keeper, and no host for a request with no Ariadne work. Its Session
      column has one link per host with a session on the request, each to the
      session panel on that host. A click opens the panel on a host that lists
      the request: one that holds Ariadne work first, then `local`.
    - An issue that many hosts list is one row, with the newest facts among
      them. Under rule 74 it names each host that lists it. Create goal opens
      the goal dialog on the picked host if that host has the repository, else
      on `local`, else on the first host that has it. The dialog has the
      repository of that host selected.
    - With one selected host, each host's rows show as they come.
    Tests in `ui/src/features/forge/forge-hosts.test.tsx`:
    “shows one row for a request two hosts list, with the newest forge facts”
    proves the match and the facts;
    “names the host of each Ariadne review and each keeper on the one row”
    proves the work labels;
    “links the session of each host that has one, each on its host” proves
    the session links;
    “opens the panel on the host that holds Ariadne work” and
    “opens the panel on local when no host holds Ariadne work” prove the click;
    “sends the filters to every host and lists a request on a later page of
    one host once” proves the filters and paging;
    “keeps the rows of the other host and the error of the host that fails”
    proves the errors;
    “shows one row for an issue two hosts list, with its newest facts” proves
    the issue match;
    “opens Create goal under the pick %j on a host with the repository” proves
    the goal dialog host and repository.
    `pull-requests-page.test.tsx` and `issues-page.test.tsx` prove one host.
    A pull request panel opens from the `?pr=` and `?host=` of its row's host.
    It shows the work of every host by rule 76.
    Tests in `ui/src/features/pull-requests/hosts.test.tsx`:
    “links the task on the request host” proves task navigation;
    “reads the selected session on the request host” proves session reads;
    “opens a session from the sessions tab on box” proves session selection;
    “shows one row for the same request on two hosts, with the session link of
    each” proves one row and its session links;
    “shows the panel host label when a named host is listed” proves panel labels.
    Local cases remain in `pull-request-panel.test.tsx` and
    `pull-requests-page.test.tsx`.
70. Start review and Start babysit have a host select, shown when more than one
    host has the request's repository registered. It is limited to those hosts,
    defaults to the panel's host, and its models, pin and ask go to the chosen
    host with that host's repository id. After the start, the panel opens the
    new session on the chosen host: `?host=` and `?pr=` move to the request on
    that host. While a host reviews the request, the panel offers no Start
    review. While a host or a task keeps it, the panel offers no Start babysit.
    Both starts stay disabled until every reachable host that has the
    repository answered or failed. A start dialog closes, and sends nothing,
    once a host reports the work it would start. Tests: “starts no … while box
    has not answered, and none once box holds the work” and “closes an open …
    dialog once box reports the work”, each for a review, a babysit and a
    babysit by a task, in `hosts.test.tsx`.
    Tests: “opens the new review console on box” and “opens the new babysit
    console on box” in `ui/src/features/pull-requests/hosts.test.tsx`;
    “opens the new session on the host Start review picked, not on the panel's
    host” in `ui/src/features/forge/forge-hosts.test.tsx`, which also proves
    the host select; “sends New turn and Stop review to the host that
    reviews, with its repository id”, “sends New turn and Stop babysit to
    the host that keeps, with its repository id” and “links the task that
    keeps the request on the host of the task” in `hosts.test.tsx` for the
    hidden starts. `pull-request-panel.test.tsx` proves local starts.
    Rule 69 gives the host of a goal dialog filled from an issue.
71. The daemon logs drawer has a host select, shown only with a named host. It
    lists the hosts of the picker, and a host the app cannot reach is a disabled
    option with its reason. It defaults to the picked host when the app can
    reach it, else to `local`, and opens the log stream of that host. The host
    the drawer opened on stays chosen until the user picks another. If it goes
    unreachable, the drawer opens no stream and says why.
72. The dashboards screen has a host select, shown only with a named host. It
    has All hosts and the hosts of the picker, and a host the app cannot reach
    is a disabled option with its reason. It defaults to the picked host when
    the app can reach it, else to All hosts, and sends no host the daemon does
    not list. A host the screen is on, picked here or taken from the header,
    stays the host while the hosts list it, reachable or not. If it goes
    unreachable, the screen says why, and the iframe keeps its `src` and does
    not reload when the host drops or returns. A change of the header pick
    changes the host only while the user has picked none on the screen. The
    iframe takes `host=` beside
    `daemon=`, and its Prometheus and Loki datasources use
    `/v1/hosts/{host}/` of the settings daemon, with `all` for the combined
    view. The shipped dashboard's series names end in `{{host}}`, which the
    combined view fills from the `host` label the daemon adds.
73. Every other screen reads `local`, and passes `"local"` explicitly.
74. One rule says where a host label shows, and `HostLabel`
    (`components/host-label.tsx`) draws every one. `useRowsShowHost()` and
    `usePanelShowsHost()` (`hooks/use-host-label.ts`) hold the rule:
    - A row shows its host when the pick is All and `GET /v1/hosts` lists a
      named host. A row of `local` then shows `local`.
    - Under one picked host, `local` included, a row shows no host. The
      attention rows, their toasts and the palette rows are not such rows: they
      come from every reachable host under each pick, so they name their host,
      `local` included, whenever a named host is listed, under any pick, as the
      panels do. With only `local` listed, none names a host.
    - The rule reads the listed hosts, not the reachable ones. A host that
      goes down removes no label from the rows that stay.
    - A saved pick that `GET /v1/hosts` no longer lists counts as All.
    - The goal, task and session panels name their host when a named host is
      listed, under any pick.
    - With only `local` listed, no row and no panel shows a host.
    The goal panel's copy menu copies `ariadne attach <id> --host NAME` for a
    goal of a named host.
75. An error of a named host names the host and gives its reason; the hint
    "Check the URL in settings" shows only for `local`. A list over many hosts
    (the goals board, the sessions screen and the agents screen) shows one
    warning per host that failed while another host answers, and an error state
    per failed host only when every host failed and no row is left. Each
    warning and error state names its host and gives its reason, and has Retry.
    A host that failed a refresh while its last read is on screen, an empty one
    too, is a warning marked "which may be outdated", never an error state.
    The board's warning for a host whose tasks failed names the host, `local`
    too, and says "may be outdated" only while a last read is on screen.
77. A goal, task and session of a named host keeps its host in what the panel
    shows and links:
    - The task panel and the session panel name their host, as the goal panel
      does (rule 74).
    - For a named host, the error titles of the goal panel, the task panel,
      the session panel, a panel's sessions list and the session activity tab
      end in `on NAME`, and the toast of a goal action (complete, cancel,
      delete) does the same.
    - A dependency row in a task's Execution summary opens the task on the
      host of the panel.
    - "Copy attach command" of a goal on a named host copies `--host NAME`
      (rule 74).
    - The request link of a session reads the host of the session. The
      sessions table and the session panel show the request's title and its
      forge link.
78. The app keeps the stream state and the last heartbeat of the stream of
    each named host, from the start of the stream to its stop, in
    `useHostStreamStore`. `local`'s stay in `useStreamStore`, and the
    connection banner stays about `local`. The picker (rule 55) and the Hosts
    table (rule 52) read them, and a host's daemon version and uptime come
    from its heartbeat.
80. The goals board and the sessions screen say which hosts their rows come
    from. With more than one listed host in the selection, each screen shows
    one line, "Not shown", that names each listed host it does not read with
    its `HostLabel` and the reason the settings daemon gives. While one host
    has not answered and a different host has, the screen shows a line "Still
    loading", naming the host that loads with its `HostLabel`. A host that failed has its
    warning by rule 75 and is in neither line.

76. The pull request panel reads the request on its own host, and on each
    reachable host that has the same forge repository, by that host's
    repository id. It shows the work of every host:
    - The header and the Ariadne fact name the host that reviews and the host
      that keeps, and "Kept by" links the task panel on the host of the task.
      The Review task and Babysit task links open the task panel on the host
      of that task.
      `HostLabel` draws each host name, where rule 74 names the panel's host.
    - Review New turn and Stop review go to the host that reviews. Babysit New turn and
      Stop babysit go to the host that keeps. Each uses that host's repository
      id.
    - The Sessions tab lists the Ariadne sessions and the task workflow
      sessions of every host. Each row names its host by rule 74, as the
      panel does. A session opens on its own host:
      `?host=` and `?pr=` move to the request on that host, so its console,
      input, cancel, answers, resume and switch go there.
    - A host that the panel cannot read shows a warning with its name and its
      reason. The work of the other hosts stays on screen.
    - With `local` alone, the panel reads `local` only and does not change.
    Tests in `ui/src/features/pull-requests/hosts.test.tsx`: “names the host
    that reviews and the task that keeps a request read on another host”,
    “links the task that keeps the request on the host of the task”, “links
    the babysit task on the host that babysits”, “sends New turn and Stop
    review to the host that reviews, with its repository id”, “sends New
    turn and Stop babysit to the host that keeps, with its repository id”,
    “lists the sessions of every host, each row named by its host, and opens
    one on its host”, and “warns of a host it cannot read, by name and reason,
    and keeps the other hosts' work”. `pull-request-panel.test.tsx` proves the
    panel with `local` alone.

79. The goal dialog sends only what the chosen host has, and says where the
    new goal is. A change of host clears the repositories, the workflow, the
    model and the effort. A chosen host whose sync is `pending` or `failed`
    shows a warning with the state and the error, and the goal can still be
    created. The toast after a create on a named host names the host. When the
    pick leaves the new goal's host out, the toast has a "Show all hosts"
    action that sets the pick to All hosts.

### Connection recovery

82. Every host name in the app is drawn with `HostLabel` or the colour dot
    (`HostDot`), in the colour of the host. `hostColor` (`lib/host-color.ts`)
    gives it: the `HostDto.color` of the host when set, else grey for `local`,
    else one of the eight `HOST_COLORS` by a stable hash of the name. The
    tokens are `--host-<name>` in `index.css`, for the light and the dark theme.
    - `HostLabel` draws the pill. `HostDot` draws the dot beside a name in the
      header picker's trigger, the logs drawer's and the Dashboards screen's
      host selects. The picker rows, the Name column of the Hosts screen and
      the `HostSync` rows draw `HostLabel`.
    - The same holds for every other host name on screen, in a title, a
      warning, a message or a toast as in a row: the attention list, the goal
      board's table and warnings, the agents screen's chips, tooltip and flags
      dialog, the models table, a repository's checkouts, the goal dialog, the
      AI card, the logs drawer's and the Permissions screen's unreachable host
      messages and the review and babysit dialogs' host select. `hostTitle`
      and `RefreshWarning` take the host as a `HostLabel`. Only a key, an id,
      the palette's search value and an accessible label are plain text.
    - A `host_updated` event repaints every one of these at once.
    - Each pill reads at WCAG AA (4.5:1) on its tint, and each dot at 2:1 on
      the page, in both themes.
    - `HOST_COLORS` lists the eight colours with their labels in a fixed order,
      and `HostColorSwatch` draws one of them.
    (`ui/src/components/host-color.test.tsx::gives a host its override, else grey for local, else a stable colour of the eight`,
    `ui/src/components/host-color.test.tsx::lists the eight colours in a fixed order with labels`,
    `ui/src/lib/host-colors.test.ts::reads each host pill at WCAG AA, and each dot against the page, in the light theme`,
    `ui/src/lib/host-colors.test.ts::reads each host pill at WCAG AA, and each dot against the page, in the dark theme`,
    `ui/src/lib/host-colors.test.ts::declares every host token in both themes and in the Tailwind theme`,
    `ui/src/components/host-color.test.tsx::draws the host select of the review dialogs with each host's dot`,
    `ui/src/components/host-color.test.tsx::draws the host name of each error, warning and message in its colour, and repaints it live`,
    `ui/src/features/system/daemon-logs-drawer.test.tsx::keeps the host it opened on when that host goes down, and says why`,
    `ui/src/features/system/daemon-logs-drawer.test.tsx::draws each host of the drawer's host select with its colour dot`,
    `ui/src/routes/dashboards.test.tsx::draws each host of the host select with its colour dot`,
    `ui/src/components/host-color.test.tsx::draws a swatch of a colour of the list`,
    `ui/src/components/host-color.test.tsx::draws a HostLabel pill and a dot in the host colour, local in grey`,
    `ui/src/components/host-color.test.tsx::repaints every place a host name is drawn when host_updated changes its colour`)

81. Settings shows the configured daemon address and the event stream connection state together. Saving the address starts a connection attempt and does not confirm success. Retry requests a stream reconnection. Settings gives daemon startup and TCP listener guidance, and identifies settings that apply immediately. Settings keeps the daemon URL, connection recovery, app diagnostics and the webhook tunnel. (`ui/src/components/settings-dialog.test.tsx::has no Hosts section`) Setup help expands with the keyboard. The live status announces changes without moving focus. The sidebar connection status keeps daemon logs reachable; host errors remain in Hosts.

83. Each goal task card shows its host in that host's colour when `host` is
    not `local`. If `host_unreachable` is true, its label says the host
    cannot be reached, and the card's accessible description says the same. A
    long host name is truncated in the label, so the task title keeps its
    room, and the description holds the full name. The label follows
    `task_updated` through the goal task list without a reload.
84. The goals board shows each task whose `goal_id` is null, from every host
    the board reads, as a group of one row (rule 39). The "Active" toggle and
    the title search narrow these rows as they narrow the goals. A board with
    no goal and no task shows the empty state alone, with its "New goal"
    action. A `task_created` or `task_updated` event adds or moves a row, also
    for a task with no goal, with no reload. A goal task and the goal task
    dialog do not change. A far task with no goal is on the board once: where
    the board reads its source host and its worker host, it shows the
    source's row and not the worker's copy.
85. The board header has one "New" button with a menu of two items. "Goal"
    opens the goal dialog and shows the `N` hint. "Task" opens the task dialog
    for a task with no goal. The command palette has a "New task" entry, which
    opens the same task dialog. Where a goal is open, the goal's own "New
    task" keeps its name and the palette's entry for a task with no goal is
    "Add a task outside any goal".
86. The dialog for a task with no goal asks for a repository, and refuses to
    create the task without one. Its workflow picker starts on "No workflow",
    its first entry. With a workflow it shows one staffing row for each column
    of that workflow. With "No workflow" it shows one staffing row with no
    column, and sends one agent with no `step` and no `workflow`. It has no
    `depends_on` field.
87. The dialog's host picker lists the `ready` hosts and starts on `local`, this
    machine. It posts to `POST /v1/tasks` of the settings daemon, with `host`
    only for a host other than `local`, and shows the daemon's reason where the
    daemon refuses the host.
88. The panel of a task with no goal has no goal breadcrumb and reads no goal.
    It shows the task's own `steps`, or no columns for a task with no workflow.

### The turn console

The session console draws a session from the daemon's trajectory (040) as
turns. It lives in `ui/src/features/sessions/trajectory/`, and shares the
composer, the form card, the status row and the find control with
`ui/src/features/sessions/console/`. The daemon folds
the events, and the console folds none: `store.ts` applies the snapshot and
the ops of each update by id.

89. The session view shows the turn console, and it reads
    `GET /v1/sessions/{id}/trajectory/stream` (rule 21)
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::shows the turn console in the session view, with no parameter`).
90. A turn shows its prompt, one activity line and its answer. A closed turn
    with nothing in its `attention` is these three rows. The activity line
    gives the time of the turn, its steps, the files it changed and its
    failed calls, as `1m 04s · 5 steps · 2 files changed · 1 failed`
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::draws a closed turn as its prompt, one activity line and its answer`).
91. The activity line is a button with `aria-expanded`. It opens to the step
    rows of the turn, which it reads from
    `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps`. A row shows the
    kind, the status, the title, the subject and the duration of its step.
    Where earlier steps exist, `Show N earlier` reads the page before the
    first row. Each row is drawn by the slot for the detail of its step
    (`trajectory/step-detail.tsx`), which rules 100 to 109 describe. A step that an
    update brought stays a row of the open fold after its turn ends, and its
    live version wins over a page read before it. The rows keep the turn's
    order: a live step that the page does not hold goes before the next live
    step that the page holds. `N` counts the earlier steps that no row shows,
    and `Show N earlier` reads the page before the first row shown
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::opens the activity line to the step rows of the turn`,
    `ui/src/features/sessions/trajectory/store.test.ts::keeps a row seen live in an open fold after its turn ends`,
    `::shows the live status of a step over a page read before it`,
    `::shows the steps of an open fold in the turn's order, each counted once`).
92. The running turn shows its last 3 steps under its activity line, and
    each new step pushes the oldest one out. Its newest message is its
    answer, and a `text_append` op extends it as it streams. The rows and
    the open fold hold the steps of the turn's own lane: the steps of a
    subagent are its own lane's
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::shows the last steps of the running turn live, and streams its text`,
    `::keeps the steps of a subagent out of its parent's last rows`,
    `ui/src/features/sessions/trajectory/store.test.ts::keeps the steps of a subagent out of its parent's live steps`).
93. A pending permission in the `attention` of a turn is a card above the
    composer, in the dock (`trajectory/dock-slot.tsx`), and never a row of
    its turn, in the last rows or in the open fold. The card shows the call,
    its subject and its options. Up, Down and 1 to 9 select an option, from
    the card and from an empty composer, and Enter or a click posts
    `{"text": "<optionId>"}` to `POST /v1/sessions/{id}/console/input`
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::draws a pending permission as a card above the composer, answered by its keys`,
    `::draws a pending permission only as its card, in the last rows and in the open fold`).
94. An open question and each error in the `attention` of a turn always
    show, whatever the fold. An open question is the form card of rule 22f,
    with its form from `GET /v1/sessions/{id}/questions`, and it posts its
    answer to the question's own route. A failed read of the questions shows
    its error with a Retry that reads them again. Where `attention_more` is
    above zero, one row gives the count, open fold or closed. It is a
    disclosure with `aria-expanded`, and it opens the steps of the turn
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::always shows an open question and each error, and counts the rest in one row`,
    `::says a failed read of the questions, and reads them again on Retry`,
    `::keeps the attention count as a disclosure once the fold is open`).
95. The composer is the one of rules 22, 22b, 22e and 23c. A sent
    prompt shows `queued` until a newer console turn holds its text: the
    prompt's `text` as it was submitted (040), which leaves out the seat
    brief, else its content where a daemon sends no `text`. A prompt that
    only ends with the same text holds another text. Each turn takes one
    queued prompt, once. A refused prompt stays with Retry and Edit, and where the
    echo of an identical send took its entry, the refusal clears the entry
    that echo left behind (rule 95). Stop and Escape post the cancel, and
    ⌘Escape moves the keyboard to the pane's Close button and closes
    nothing (rule 23). The slash commands come from the `commands` of the
    session's state. The draft and the refused prompts are retained in memory
    per daemon address and session, as rule 99g says: another session starts
    with its own, and a refusal that lands after the pane moved goes to the
    session that sent it
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::sends a prompt as queued, and drops the queued block once a turn holds it`,
    `::keeps a refused prompt with Retry and Edit`,
    `::keeps the draft and the refused prompts apart for each session`,
    `::cancels a running turn with Stop and with Escape`,
    `::moves the keyboard to the pane's Close button on ⌘Escape, and closes nothing`,
    `::lists the agent's slash commands from the session state`,
    `ui/src/features/sessions/trajectory/store.test.ts::keeps a queued prompt until a newer turn holds its text`,
    `::takes one queued prompt per echo, whatever revision follows`,
    `::takes a queued prompt away when its echo carries the seat brief ahead of it`,
    `::keeps a queued prompt when another prompt only ends with its text`,
    `::clears the entry an identical send's echo left behind when a send is refused`).
96. A scroll to the top reads the whole turns before the oldest one, with
    `GET /v1/sessions/{id}/trajectory?before=<turn id>`, while `has_older`
    is true. Turns that fit the box leave no scroll, so they read the older
    turns until the box scrolls or no older turn is left. The page starts at
    the oldest turn of the history read whole. A turn the stream brings from
    before it, such as an old turn whose error a diagnosis names, is held but
    not shown until the pages reach it. Such a header can carry no prompt
    and no answer. The turn then keeps the stream's state and attention and
    the page's prompt and answer, whichever of the two comes first. A later
    header with no prompt or answer keeps the ones the reader loaded. The
    reader's place holds. The console follows the newest output while the
    reader is at the bottom, and shows `Jump to latest`, marked `New output`,
    once the reader scrolls away
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::loads older turns at the top and keeps the reader's place`,
    `::loads older turns while the turns fit the box and no scroll can reach the top`,
    `::reads older turns from the history it holds, not from an old turn the stream brought`,
    `::follows the newest output, and offers Jump to latest once the reader scrolls away`,
    `ui/src/features/sessions/trajectory/store.test.ts::keeps a turn the stream brings from before the history out of view and off the cursor`,
    `::keeps the page's prompt and answer, and the stream's newer attention, when the stream came first`,
    `::keeps the prompt and answer the reader loaded when a partial header comes after the page`).
97. A `resync` or a drop reconnects on the event stream's backoff. The new
    snapshot replaces the state and drops the older turns and the step pages
    loaded before it, so no item shows two times. An update whose `rev` the
    console holds already changes nothing
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::replaces the state on a reconnect and shows no item two times`,
    `ui/src/features/sessions/trajectory/store.test.ts::replaces a turn and a step by id, so an upsert never shows an item two times`,
    `::ignores a revision it has applied already`,
    `::replaces the state with a snapshot and drops the pages loaded before it`).
98. The turn console uses the app's tokens in both themes. No source of it
    names a fixed colour, and nothing it draws shows one, in the light or the
    dark theme, the shared parts of `console/` included. The shared form
    card's marker takes the token `--question-fg`, which the console sets
    on the card's wrapper
    (`ui/src/features/sessions/trajectory/tokens.test.ts::names no fixed colour in any source of the turn console`,
    `ui/src/features/sessions/trajectory/trajectory-console.test.tsx::draws every part with the app's tokens, shared cards included, in both themes`).
99. The status row (`trajectory/status-slot.tsx`), the dock above the
    composer (`trajectory/dock-slot.tsx`) and the find control
    (`trajectory/find-slot.tsx`) are named mount points, each in its own
    file. The status row shows the session's status, the running turn and
    the token totals (rule 99b)
    (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx::shows the session's status, the running turn and the token totals in the status row`).

#### The live state of the session

Rules 99a to 99h show the live state of the session outside the activity
fold, and keep the reader's view. The data comes from the session state, the
plans and the `note` and `compaction` steps of the trajectory (040).

99a. While a plan has an open entry, one plan line shows in the dock above
    the composer (`trajectory/plan.tsx`). It is the newest such plan, as
    `Plan <done>/<all> · <current entry>`. The current entry is the first
    `in_progress` entry, else the first entry not `completed`. The line is a
    disclosure with `aria-expanded`, and it opens to the checklist of the
    entries: `✓` done, `●` in progress, `○` the rest. A plan with every entry
    `completed` leaves the dock and shows in the turn that last changed it,
    as the same line, closed. A removed plan and a plan with no entries show
    nowhere
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::shows one plan line above the composer while a plan has an open entry, and a finished plan in its turn`).
99b. The status row shows, in this order, the status, the running turn, the
    mode, each config option with its current value, the context use as a
    share of the window, the cost in its currency, and the token totals at
    its right end. The row is one line at each width: it does not wrap, and
    a fact that does not fit is cut. The row does not count the running
    subagents, and it has no bar of views: the console of events said
    `running N subagents`, and that ends here. A running subagent shows as its
    own step row, with its state and its step count (rule 106)
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::counts no running subagent in the status row, and draws no bar of views for one`,
    `ui/src/features/sessions/trajectory/console-state.test.tsx::shows the status, the mode, the options, the context use, the cost and the tokens in one line`).
99c. A warning note and a compaction always show as lines of their turn,
    whatever the fold, and an error shows as rule 94 says. The console takes
    the warning notes and the compactions from the `notices` field of the
    turn (040). It reads no page of steps to find them. A compaction line
    says it runs, ended, or failed with its error. An info note is a row of
    the open activity fold alone: it is not a line of the turn, nor one of
    the last rows of a running turn (`trajectory/notices.tsx`)
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::shows the warnings and the compaction of a closed turn from its notices, and reads no steps for them`,
    `ui/src/features/sessions/trajectory/console-state.test.tsx::always shows a warning, an error and a compaction in their turn, and an info notice in the fold alone`).
99c1. Where `notices_more` is above zero, one row gives the count, open fold
    or closed, as `N more notices`. It is a disclosure with `aria-expanded`,
    and it opens the steps of the turn. In an open fold, a warning or a compaction among the
    steps read shows as a line of the turn too, once, in time order, ties by step ID
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::counts the notices of a turn that its notices do not hold in one row, which opens the steps of the turn`).
99d. An answered permission is a mark on the row of its call, found by the
    call's `permission` id or by its tool call id, and not a row of its own
    where the call's row shows. The mark gives `✓` or `✗` by the kind of the
    option chosen, the option's label, and who decided: `you`, `AI`, a
    learned rule or `auto`
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::marks an answered permission on the row of its call, with who decided`).
99e. The title that the agent gives the session shows as the subtitle of the
    console, under its stream line, and a `session_patch` changes it
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::shows the title that the agent gives as the subtitle of the console`).
99f. Find (`trajectory/find-slot.tsx`, over `console/transcript-find.tsx`) searches the loaded turns and the
    loaded steps: the prompt and the answer of each turn shown, and each
    step the console holds for it, including live steps, full pages, attention
    and the notices of the turn. Each step appears once by ID.
    Find reports when older turns are unloaded and loads them on request
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::finds the text of the loaded turns and the loaded steps`,
    `ui/src/features/sessions/trajectory/console-state.test.tsx::shows the warnings and the compaction of a closed turn from its notices, and reads no steps for them`).
99g. The console expands into a modal and collapses back. Expand, collapse and a change of tab each
    mount a new pane. The console keeps the draft, the scroll place and
    whether it follows, and the open folds of each session, the turns' and
    the plans', in memory and per daemon address and session
    (`console/console-view-state.ts`). Another session or another daemon starts
    with its own, and a browser restart forgets them. A new pane reads them
    back, and each console on screen holds one stream
    (`ui/src/features/sessions/console/composer.test.tsx::brings a session's draft back after the console is unmounted`,
    `::keeps each daemon's and each session's state apart`,
    `::keeps a named host's state when its address changes, and gives the host's reason while it is down`),
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::keeps the draft, the scroll place and the open folds through expand and collapse`,
    `ui/src/features/sessions/trajectory/console-state.test.tsx::keeps the draft, the scroll place and the open folds through a change of tab`).
99h. A pane mounted again reads the older turns back to the oldest turn the
    console had loaded. It restores each open fold's loaded step range
    before it restores the scroll offset. This keeps the older rows
    loaded by Show earlier in the open fold
    (`ui/src/features/sessions/trajectory/console-state.test.tsx::loads back to the oldest turn it had loaded before it puts the reader's place back`,
    `ui/src/features/sessions/trajectory/console-state.test.tsx::puts the place back once the open folds read their rows, not before`).

#### What the console of events had and the turn console does not

These behaviors of the console of events (rules 21 to 24i before this
change) end. Each has the place that now serves its need.

- A bar of views for a subagent, and a view of one child: the subagent is a
  step row that opens to its own steps (rule 106). The `main` lines per child
  announcement and end, and the status row's `running N subagents`, end.
- Escape cancelling a running child that may be cancelled, and Escape in a
  child's view going back to `main`: Escape cancels the running turn, or
  closes a full view (rules 95 and 108). A child is cancelled from the
  Cancel control of its row, where `can_cancel` is true (rule 106).
- Runs of activity rows cut to their last 3 under `Show N earlier`, and the
  per-run choice kept in the view state: the running turn shows its last 3
  steps, and an open fold shows the steps read, with `Show N earlier`
  (rules 91 and 92).
- Selecting a tool or permission row to read its details beside the
  conversation, and the `Pending request` control: a step opens under its
  row, and `Open` shows it whole in the full view (rules 100 to 108). A
  pending permission is a card above the composer (rule 93).
- Folds of single thoughts and answered questions by block id, and a footer of
  keys: a turn's fold is the one fold, and the status row shows the state.
- Rows for `agent_request` events, `kind · summary` rows for unknown kinds and
  system notes drawn from raw events: the trajectory holds typed steps
  (040), and a `note` step is a row of the open fold (rule 99c).
- The command list read from `available_commands_update` events, and the
  older pages read from `GET /v1/events`: the commands come from the
  session state, and older turns from the trajectory route (rules 95 and 96).

### A step in place

A step of the turn console opens under its row, and shows its detail by its
kind. `trajectory/step-detail.tsx` draws the row and the detail,
`trajectory/step-row.tsx` the row, `trajectory/content-blocks.tsx` the
content blocks that are not text, and `trajectory/full-view.tsx` the full
view.

100. A step row is a button with `aria-expanded`. A click, Enter or Space
     opens the detail of the step under the row, to a set height, and the
     same action closes it. The rows of an open fold, the last rows of a
     running turn and the errors of a turn's `attention` all open this way
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::opens a step under its row on a click, Enter or Space, to a set height, and closes it the same way`).
101. The detail of a tool call is read from
     `GET /v1/sessions/{id}/trajectory/items/{item_id}` and draws by its
     `type`. A `command` shows the command and its output, with the working
     directory and the exit code. A `diff` shows each file in the task diff
     viewer (`features/tasks/diff-editor.tsx`), with its line numbers and the
     colours of its language. A `file` shows the text read with its language
     and its line numbers from `line`. A `search` and a `fetch` show the query
     or the URL, and the result. A `generic` detail shows its content blocks
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::draws a command as its command and its output`,
     `::draws an edit as a diff with line numbers and language colours`,
     `::draws a read as the file with its language and its line numbers`,
     `::draws a search and a fetch as the query or the URL, and the result`).
102. The content of a detail comes first and its controls second. Each code
     section is a `CodeBlock` under one frame of its own, never a frame in a
     frame, with its name and its language. `Copy` in a section copies the
     content of that section
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::copies the content of a section`).
103. `Raw` is a toggle with `aria-pressed`. It reads the detail again with
     `raw=true` and shows the raw input and the raw output as JSON. The
     detail reads without `raw` until then, and no raw data shows without it.
     Each step the full view shows starts afresh, with `Raw` off, so the raw
     data of one step never shows under another
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::shows the raw input and the raw output only behind Raw`,
     `::starts each step the full view shows afresh, with Raw off`).
104. An image block shows inline, in the answer of a turn and in a detail.
     An audio block, a resource and a resource link show as a labelled row
     with a link. A block with bytes links to its `blob` path on the host's
     daemon (040)
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::shows an image inline in a message and in a detail, and audio and resources as labelled rows with a link`).
105. The detail of a tool call is read again at each new `detail_rev` of
     its step, so an open step of a running call shows new progress, in
     place and in the full view. The store keeps the live version of each
     step of a subagent's lane (`children`), which a snapshot drops, and an
     open subagent shows it over the page it read. So a running call of a
     subagent shows new progress while the subagent's step count holds.
     The full view reads the selected step from the store even when its
     inline row closes or leaves the conversation.
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::reads the detail of an open running call again at each new detail_rev`,
     `::shows the newest progress of a running call in the full view`,
     `::shows the new progress of a subagent's running call while its step count holds`,
     `::keeps showing child tool progress after its inline subagent collapses`,
     `ui/src/features/sessions/trajectory/store.test.ts::keeps the live version of each step of a subagent's lane, and a snapshot drops them`).
106. A subagent step row shows its title, its task, its state, its time and
     the count of its steps. It opens to the subagent's own steps, read from
     `GET /v1/sessions/{id}/trajectory/turns/{turn_id}/steps?subagent=<id>`,
     and each of those opens in place too. The newest page is read again at
     each new step of the subagent, and a row once read stays. Where earlier
     steps exist, `Show N earlier` reads the page before the first row, with
     `before=<step id>`, in place and in the full view. Where `can_cancel` is true, the
     row has a Cancel control that posts `{"subagent_id"}` to
     `POST /v1/sessions/{id}/console/cancel`. Where it is false, the row has
     none
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::draws a subagent step with its title, task, state, time and step count, opens it to its steps, and cancels it`,
     `::offers no cancel on a subagent the agent does not let cancel`,
     `::reads the earlier steps of a subagent with Show N earlier`).
107. `Open` shows the whole content of a step, with no set height, in the
     full view. On a console panel of 880 CSS pixels or wider the full view
     sits beside the conversation, and on a narrower panel it is a modal
     drawer. The width is the panel's, not the browser's. The full view of a
     subagent shows its whole work: its steps
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::opens the whole step beside the conversation on a panel of 880 px or wider`,
     `::opens the whole step in a drawer on a panel narrower than 880 px`).
108. The full view takes the keyboard on its Close button, for each step it
     shows: an `Open` inside the view shows another step there, and the
     keyboard stays in the view. Escape in the full view closes it and does
     not cancel the running turn. The keyboard goes back to the control
     outside the view that opened it, else to the row of the step shown,
     else to the console. Escape elsewhere in the console still cancels the
     turn (rule 95)
     (`ui/src/features/sessions/trajectory/step-detail.test.tsx::closes the full view on Escape and does not cancel the running turn`,
     `::keeps the keyboard in the full view when an Open inside it shows another step, and gives it back to the first Open`,
     `::returns focus to the step row when Escape closes a drawer whose opener disappeared`).
109. The step detail and the full view use the app's tokens alone, as rule
     98 says of the whole turn console
     (`ui/src/features/sessions/trajectory/tokens.test.ts::names no fixed colour in any source of the turn console`).

## Acceptance criteria

- Goal task cards show a remote host in its host colour and explain when it
  cannot be reached, in the label and in the card description (`ui/src/features/tasks/task-card.test.tsx::shows the colored remote host and its unreachable state`).
- A goal task list follows a `task_updated` change to its host and reachability
  without a reload (`ui/src/features/tasks/task-card.test.tsx::updates the goal task host label from task_updated without a reload`).

- Each goal-less task is a group of one row, in no goal's group
  (`ui/src/features/goals/goal-board.test.tsx::draws each goal-less task as a group of one row, in no goal's group`).
- A far goal-less task that its source and its worker both list is on the board once
  (`ui/src/features/goals/goal-board.test.tsx::shows a far goal-less task once, though its source and its worker both list it`).
- A `task_created` event adds a goal-less task's row, and a `task_updated` event moves a row into its goal
  (`ui/src/features/goals/goal-board.test.tsx::adds a goal-less task's row when its task_created event arrives`; `::moves a task's row into its goal when its task_updated event arrives`).
- The board shows goal-less rows with no goal, keeps them when the goals request fails, and shows the empty state alone with no goal and no task
  (`ui/src/features/goals/goals-list-page.test.tsx::shows the rows of goal-less tasks on a board with no goals at all`; `::keeps the rows of goal-less tasks when the goals request fails`; `::shows only the empty state on a board with no goals and no tasks`; `ui/src/features/goals/goal-board.test.tsx::shows only the empty state when the board has no goal and no task`).
- The header has one New menu: Goal opens the goal dialog and shows the `N` hint, and Task opens the goal-less task dialog
  (`ui/src/features/goals/goals-list-page.test.tsx::offers one New menu: Goal opens the goal dialog with its N hint, Task the goal-less task dialog`; `::opens the goal-less task dialog from the Task item of the New menu`).
- The palette offers New task for a task with no goal, and a distinct entry beside a goal's own
  (`ui/src/features/command-palette/command-palette.test.tsx::offers a task with no goal over a screen with no goal open, and opens its dialog`; `::offers the goal-less task beside the goal's own when a goal is open`).
- The dialog offers "No workflow" first with one row and no column, one row per column with a workflow, no dependency field, and the ready hosts starting on this machine
  (`ui/src/features/tasks/create-standalone-task-dialog.test.tsx::offers No workflow first, and one staffing row with no column under it`; `::staffs one row for each column of the picked workflow`; `::has no dependency field`; `::offers the ready hosts, starting on this machine`).
- The dialog posts the repository, the one agent and no workflow, and refuses a missing repository
  (`ui/src/features/tasks/create-standalone-task-dialog.test.tsx::creates the task in the repository with no workflow, on this machine`; `::asks for a repository before it creates anything`; `::shows the daemon's reason when it refuses the host`).
- The panel of a goal-less task has no goal breadcrumb or goal query, and no columns without a workflow
  (`ui/src/features/tasks/task-panel.test.tsx::draws a goal-less task by its own columns and reads no goal`; `::shows no columns for a goal-less task with no workflow`).

- Settings shows the configured address, stream state and recovery guidance: `ui/src/components/settings-dialog.test.tsx::shows the saved address and live connection state with recovery guidance`; `::announces stream changes without moving focus and gives neutral failure guidance`.
- Pinning reserves space without a scrim, and mode changes preserve content
  (`ui/src/components/pinned-inspector.test.tsx::reserves overview space only when pinned and preserves content through fallback and expansion`).
- The icon-only pin control exposes `aria-pressed`, an accessible name of
  Pin inspector or Unpin inspector, and the same text in its native title tooltip
  (`ui/src/components/pinned-inspector.test.tsx::shows an icon-only pin control without focus buttons`).
- Nested confirmations remain modal and own Escape
  (`ui/src/components/pinned-inspector.test.tsx::keeps nested confirmations modal and gives Escape to the dialog first`).


- A task card of a named host links to its panel on that host
  (`ui/src/features/tasks/task-card.test.tsx::links a local card to its panel and a remote card to its panel on its host`).
- The task panel reads a named host, its stream refreshes the task, and
  retry posts there
  (`ui/src/features/tasks/hosts.test.tsx::reads the task of a named host and refreshes it on that host's stream`,
  `::retries a failed task on the host the panel is open on`).
- The sessions screen merges hosts, and a remote session's console streams,
  takes input and cancels on its host
  (`ui/src/features/sessions/hosts.test.tsx::merges the sessions of every host, each row naming its host, and opens one on its host`,
  `::applies the picked host to the list and names no host`,
  `::titles each row from its own host when two hosts share a goal and task id`,
  `::opens the trajectory stream of a remote session on its host and sends input there`,
  `::cancels a turn of a remote session on its host and shows the host's stream result`).
- One rule shows the host of a row and a panel (rule 74): under All with a
  named host, under one pick, with a host down, under a saved pick no longer
  listed, and with only `local`
  (`ui/src/hooks/use-host-label.test.tsx::shows the host on rows and panels under All with a named host listed`,
  `::shows no host on rows under one picked host, but still on panels`,
  `::shows no host on rows when local is the picked host`,
  `::keeps the host on rows when a named host goes down`,
  `::counts a saved pick the daemon no longer lists as All`,
  `::shows no host on rows or panels with only local listed`).
- The goals on the board follow rule 74
  (`ui/src/features/goals/hosts.test.tsx::merges the goals of every host under All, each goal naming its host`,
  `::reads only the picked host, and names no host on its goals`,
  `::names each goal's host under a saved pick the daemon no longer lists`,
  `::keeps the host of the goals that stay when a named host goes down`,
  `::names no host on the goals with only local listed`).
- The goals board and the sessions screen say which hosts are absent or
  still loading (rule 80)
  (`ui/src/features/goals/hosts.test.tsx::says which listed host is not shown and why`,
  `::names the host that still loads while another has answered`,
  `ui/src/features/sessions/hosts.test.tsx::says which listed host is not shown and why, with host badges on the rows`,
  `::names the host that still loads while another has answered`).
- The goal and task chips keep their host, apply only while it is selected,
  and Clear filters removes them (rule 61)
  (`ui/src/features/sessions/hosts.test.tsx::keeps the host of a goal chip and applies it only while that host is selected`,
  `::takes the host of a goal picked in a row, and Clear filters removes both chips`).
- The board says the tasks of a host are not loaded, and its warning names the
  host (rules 58 and 75)
  (`ui/src/features/goals/hosts.test.tsx::says the tasks of a host are not loaded in its goals, and names the host, local too`,
  `::says the last read may be outdated only while one is on screen`,
  `::keeps the goals and says tasks are not loaded when every host's task read fails`,
  `::shows the goals as not loaded while every task read is pending`).
- The attention rows name their host (rule 59)
  (`ui/src/features/goals/hosts.test.tsx::names local on a local attention row under All`,
  `::lists the attention rows of every host under one picked host, each naming its host`).
- The attention list reads every reachable host, toasts name a named host,
  one host's error leaves the others, a host's first answer is silent, and an
  agents item opens its host's agents
  (`ui/src/features/goals/attention-hosts.test.tsx::counts the items of every reachable host under a pick of one`,
  `::names the host on the toast of an item of a named host`,
  `::names local on the toast of a local item while a named host is listed`,
  `::names no host on a toast with only local listed`,
  `::names the failed host with Retry, keeps the other items and keeps announcing`,
  `::raises no toast for the first answer of a host that becomes listed`,
  `::raises no toast for what a returning host raised while it was away, once its fresh answer lands`,
  `::opens the agents of a named host with the pick set to All hosts`,
  `ui/src/features/command-palette/hosts.test.tsx::opens the agents of a named host from Needs attention with the pick set to All hosts`).
- The goal panel names its host and copies `--host`
  (`ui/src/features/goals/hosts.test.tsx::names the goal panel's host, local included, while a named host is listed`,
  `::names no host in the goal panel with only local listed`,
  `::copies the attach command of a goal of a named host with its host`).
- The task panel and the session panel name their host
  (`ui/src/features/tasks/hosts.test.tsx::names the host of the task panel under any pick while a named host is listed`,
  `ui/src/features/sessions/hosts.test.tsx::names the host of the session panel while a named host is listed`).
- A goal, task and session of a named host keep their host (rule 77)
  (`ui/src/features/goals/hosts.test.tsx::names the host in the title of a goal that failed to load there`,
  `::names the host in the toast of a goal action on a named host`,
  `::copies the attach command of a goal of a named host with its host`,
  `ui/src/features/tasks/hosts.test.tsx::names the host of the task panel under any pick while a named host is listed`,
  `::names the host in the title of a task that failed to load there`,
  `::opens a dependency of a task of a named host on that host`,
  `ui/src/features/sessions/hosts.test.tsx::names the host of the session panel while a named host is listed`,
  `::names the host in the titles of a session that failed to load there`,
  `::names the host in the title of a sessions list that failed on a named host`,
  `::names the host in the title of an activity tab that failed on a named host`,
  `::reads the request link of a pull request session on the session's host`,
  `::reads the request link of a pull request row of the sessions table on its host`).
- The session rows follow rule 74 under a saved pick no longer listed
  (`ui/src/features/sessions/hosts.test.tsx::names each row's host under a saved pick the daemon no longer lists`).
- The issue rows follow rule 74
  (`ui/src/features/forge/forge-hosts.test.tsx::merges the issues of every host, and names the host of each`,
  `::names local on a local issue row under All, and keeps it when a named host goes down`,
  `::names no host on an issue row under one picked host`).
- The agents screen keeps the chip of the hosts that stay
  (`ui/src/features/agents/agents-hosts.test.tsx::groups an agent of two hosts under one tab, with a chip per host, and writes to the chip's host`,
  `::reads only the picked host, with no chips`,
  `::keeps the chip of local when a named host goes down`).
- The palette rows name their host (rule 64), and "New task" reads the open
  goal on its host
  (`ui/src/features/command-palette/hosts.test.tsx::names local on a local row under All`,
  `::names the host of every row under one picked host`,
  `::offers New task for the open goal on its own host under another pick`).
- The palette searches every reachable host, opens a row on its host and
  copies `--host`
  (`ui/src/features/command-palette/hosts.test.tsx::lists the tasks of every selected host, and opens a row on its own host`,
  `::searches the hosts outside the pick too`,
  `::copies the attach command with the host of the open session`,
  `::copies the attach command of a local task with no host flag`).
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
  (`ui/src/components/pane-focus.test.tsx::returns focus to the board opener after closing the replacement %s`,
  run for `task` and for `goal breadcrumb`).
- Removing an action or closing an expanded view restores focus inside the pane
  (`ui/src/components/pane-focus.test.tsx::keeps focus in the pane after confirming %s`,
  run for `Cancel goal` and for `Cancel task`,
  `::keeps focus in the pane after Retry task removes its button`,
  `::keeps focus in the pane after collapsing the %s`, run for `console` and
  for `diff`).
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
  (`::lists display names in the picker, falls back to metadata names, and
  keeps the id in the iframe`); Reset is disabled on the shipped document and enabled once
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
- The task panel has no messages tab and no message input, reads no
  message route, and a `message_sent` event changes nothing on it
  (`ui/src/features/tasks/task-panel.test.tsx::offers no messages tab and no message input, and reads no message route`,
  `::changes no view and logs no error when the stream carries a message_sent event`).
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
- The composer grows to six lines, sends from Enter and from Send, and keeps
  a newline, a composition and a paste (rule 22b)
  (`ui/src/features/sessions/console/composer.test.tsx::starts at one line, grows with its text to six lines, then scrolls inside`,
  `::sends with Enter and the Send control, and keeps a newline, a composition and a paste`,
  `::offers Stop beside Send while a turn runs`,
  `::draws Send and Stop as icons without a label, on a row that never wraps`).
- A form question draws its message, fields and choices as written, in every
  field kind (rule 22f)
  (`ui/src/features/sessions/console/form-card.test.tsx::draws one question at a time with large choice rows`,
  `ui/src/features/sessions/console/form-questions.test.ts::reads each field of the schema with its choices as the agent wrote them`).
- A form question is sent as accept with its content, or dismissed as decline,
  on its own route (rule 22f)
  (`ui/src/features/sessions/console/form-card.test.tsx::sends the answers as accept with the content of the form`,
  `::sends decline when the user dismisses the question`,
  `::shows a failed answer on the card and lets the user try again`).
- The attention strip answers a one-field single-select form in place, and
  opens the console for any other (rule 59)
  (`ui/src/features/goals/attention-forms.test.tsx::answers a form of one single-select question in place`,
  `::opens the console of the session for a form it does not answer in place`).
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
  session ended and Switch only where it can, and
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
  `::omits global fold actions from the menu`).
- Code in the console and in Markdown is a `CodeBlock`: a fence names its
  language, the highlight loses no text, and one light frame holds a fence
  (rule 23a2)
  (`ui/src/lib/highlight.test.ts::names a language by its fence or its extension, and labels it`,
  `::leaves a name the data does not know, or plain text, unknown`,
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
- The console retains a session's draft, folds and place apart per daemon and
  session, and keeps one open stream while shown (rule 99g)
  (`ui/src/features/sessions/console/composer.test.tsx::brings a session's draft back after the console is unmounted`,
  `::keeps each daemon's and each session's state apart`,
  `::keeps a named host's state when its address changes, and gives the host's reason while it is down`,
  `::holds one open stream for the console shown, and none once it is hidden`).
- A prompt and a developer prompt render as
  Markdown, with a single newline kept and a fence a `CodeBlock` (rule 21)
  (`ui/src/features/sessions/console/blocks.test.tsx::renders a prompt as Markdown: headings, task lists, a fenced block, and a single newline still breaks`,
  `::renders developer prompts as full Markdown without a fold`,
  `::draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text`).
- The console expands into a near-fullscreen modal and collapses back, and
  ⌘Escape moves focus to the Close button (rule 23)
  (`ui/src/features/sessions/trajectory/console-state.test.tsx::keeps the draft, the scroll place and the open folds through expand and collapse`,
  `ui/src/features/sessions/trajectory/trajectory-console.test.tsx::moves the keyboard to the pane's Close button on ⌘Escape, and closes nothing`,
  `ui/src/components/panel-sheet.test.tsx::leaves Escape on the board alone and returns focus when the pane closes`).
- The status row cuts a multiline tool name to its first line, whole on hover,
  with the tokens at the right end (rule 99b)
  (`ui/src/features/sessions/console/status.test.tsx::shows only the first line of a multiline tool name, whole on hover, with the tokens at the right end`).
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
  (`ui/src/features/permissions/ai-test-panel.test.tsx::sends manually edited title, kind, options and locations one per line`,
  `::shows a field error on invalid JSON and disables Test`). These typing
  proofs wait for the dialog's opening focus to reach Examples before editing,
  so its deferred focus cannot interrupt the simulated keystrokes under load.
- The board draws each task as one row of one list, the rows of a goal in its
  group, and counts them; Active hides terminal tasks without changing
  progress
  (`ui/src/features/goals/goal-board.test.tsx::draws each task as one row of one list, the rows of a goal in its group, and counts them`,
  `::hides terminal tasks under Active while preserving progress`).
- The board has no line of column names
  (`ui/src/features/goals/goal-board.test.tsx::draws each task as one row of one list, the rows of a goal in its group, and counts them`).
- A task row is one link to the task panel with one tab stop, its PR link
  outside the link
  (`ui/src/features/goals/goal-board.test.tsx::makes a row one link to the task panel with one tab stop, its pull request outside the link`).
- A task row has no border and no fill, hover and keyboard focus tint it,
  and its rail segment takes the card's border colour, a goal-less task's
  too
  (`ui/src/features/goals/goal-board.test.tsx::draws a row with no border and no fill, its rail segment in the status colour`).
- An attention item is a block below the task row or goal line it names:
  below each task it names or whose session it names, one below each of many
  tasks, below the goal line for the goal or a session with no task, below the
  row of a task with no goal, and none under a folded goal, whose line keeps
  the badge
  (`ui/src/features/goals/attention-blocks.test.tsx::shows a block below the row of the task an item names, and none below the others`;
  `::shows a block below a task whose session an item names`;
  `::shows one block below each task an item names, and one if it names a task and its session`;
  `::shows a block for the goal, or for a session of the goal with no task, directly below the goal line`;
  `::shows a task with no goal its blocks below its row`;
  `::shows no block under a folded goal, and its goal line keeps the attention badge`).
- A block draws the content of a drawer row and keeps every link and button
  in the tab order
  (`ui/src/features/goals/attention-blocks.test.tsx::draws the content of a drawer row: summary, action, age, links, target and Dismiss`;
  `::keeps every link and button of a block in the tab order`).
- A block of a form of one single-select question answers it with one button
  for each choice, and a block of any other form links to the console
  (`ui/src/features/goals/attention-blocks.test.tsx::answers a form of one single-select question from its block with one button per choice`;
  `::links a form of another shape to the console and draws no buttons`).
- The blocks read only the shared list, also while its read is incomplete
  (`ui/src/features/goals/attention-blocks.test.tsx::shows blocks from the shared list while the read is incomplete`;
  `ui/src/features/goals/attention-incomplete.test.tsx::retains known items and their board badges while reporting an incomplete read`).
- The token figures share one column sized to the largest figure, the
  goal's total directly above its rows', and no cell cuts a figure
  (`ui/src/features/goals/goal-board.test.tsx::puts the token figures in one column, the goal's total over its rows', never cut`).
- From 33.9rem wide up, the board never scrolls sideways; below that it
  scrolls sideways and cuts no column. It hides whole columns by its own
  container width: step names, then last activity, host and PR link, all
  four at a pinned inspector's width; the title, state, step dots and tokens
  stay, and the title ends in an ellipsis
  (`ui/src/features/goals/goal-board.test.tsx::hides whole columns by the board's own width: step names, then activity, host and PR link`,
  `::hides a goal's host on its goal line with the host column`).
- The state, the step dots and the tokens never shrink below their content,
  the step column's floor being the widest dots of any workflow, folded or
  not; only the title gives up its width
  (`ui/src/features/goals/goal-board.test.tsx::keeps the state, the step dots and the tokens whole: only the title gives up its width`,
  `::sizes the dots floor to the widest track when a folded workflow is the longest`,
  `::sizes the dots floor to an unfolded workflow of five steps`).
- The groups sort by band, newest change first, goal-less tasks by the same
  rule
  (`ui/src/features/goals/goal-board.test.tsx::sorts the groups: what needs the user, then live, then finished, newest change first`).
- Finished, cancelled and failed cards use their status borders
  (`ui/src/features/tasks/task-card.test.tsx::outlines a %s task with its status color`).
- A goal line is one line of chevron, title link, status, `done/total`,
  orchestrator badge and tokens over the rows of its goal, shown at rest,
  with no background, border or frame; a folded goal is the goal line with
  its summary; a goal with no shown task is the goal line and one muted
  state line, and unfolding to it leaves the keyboard on the chevron
  (`ui/src/features/goals/goal-board.test.tsx::puts one goal line over the rows of its goal, shown at rest: toggle, title link, status, progress and tokens`,
  `::shows the orchestrator's attention badge on the goal line`,
  `::folds a finished goal to its goal line with the summary, and no border`,
  `::draws a goal with no shown task as its goal line, then one muted line with the state`,
  `::hands the keyboard to the fold button when unfolding leaves a goal with no shown task`).
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
  `::keeps cached table rows under the warning, and shows a load error with none`).
- The goals screen narrows the board by title and names the search when nothing matches.
- The console shows the plan, the session state, the notices, the
  marks of answered permissions, the title and find, and keeps the view of
  each session (rules 99a to 99h) (`ui/src/features/sessions/trajectory/console-state.test.tsx`).
- The console (rules 89 to 99) draws a closed turn as three rows, opens its activity line to step rows, shows the
  running turn live, answers a pending permission from a card above the
  composer, always shows the attention of a turn, keeps the composer of
  rules 22 to 23c, loads older turns and keeps the place, and replaces its state on a
  reconnect with no item two times, in the app's tokens alone
  (`ui/src/features/sessions/trajectory/trajectory-console.test.tsx`,
  `ui/src/features/sessions/trajectory/store.test.ts`,
  `ui/src/features/sessions/trajectory/tokens.test.ts`).
- A step of the turn console (rules 100 to 109) opens under its row, draws
  its detail by kind, shows raw data only behind `Raw`, copies a section,
  shows images inline and other blocks as linked rows, follows a running
  call, opens a subagent to its steps with a cancel where permitted, and
  opens whole in a full view beside the conversation or in a drawer, which
  Escape closes without a cancel
  (`ui/src/features/sessions/trajectory/step-detail.test.tsx`).

- Hosts (rules 51 to 60): `local` maps to the settings base URL and a named
  host is reachable only over HTTP while ready
  (`ui/src/api/hosts.test.tsx::maps local to the settings base URL, whatever endpoint it lists`,
  `::lists a host whose endpoint is not http or https as unreachable, with its error`,
  `::lists a host that is not ready as unreachable, even at an http endpoint`,
  `::calls a ready named host at its endpoint`,
  `::builds a host's stream URL with its filters, and none for an unreachable host`,
  `::reads GET /v1/hosts from the settings daemon, and points api(host) at each host`,
  `::reads a host again at its new address once local's stream comes back`);
  the Hosts screen lists the facts live, and the picker picks and keeps a
  host (`ui/src/features/hosts/hosts-page.test.tsx::lists every host with its name, kind, status, endpoint and error, live through host_updated`,
  `::picks All, local or a named host with its status, and keeps the pick`,
  `::shows no host picker while the daemon lists local alone`,
  `::shows the host picker for a named host that is down`,
  `::shows a host with a socket path as not reachable, never as ready`,
  `::shows the status of a picked named host in the trigger`,
  `::reads a saved pick the daemon no longer lists as All hosts`);
  each named host has a reconnect button in its picker row and its Hosts row,
  and `local` has none
  (`ui/src/features/hosts/hosts-page.test.tsx::reconnects a named host from each picker row, in each status, and picks nothing`,
  `::reconnects a host from its Hosts row, disabled while it runs, and shows the new status`,
  `::shows the error of a reconnect that fails`,
  `::shows no reconnect button while the daemon lists local alone`);
  the Hosts table and the picker show the true state of each host: reach with
  its reason, daemon version and uptime, the app's stream state, the sync, and
  the mark on All hosts
  (`ui/src/features/hosts/hosts-page.test.tsx::shows the reach of a socket-path host and its reason in the words of the picker`,
  `::shows the daemon version and uptime of each reachable host, from its stream's heartbeat`,
  `::shows the app's stream state of a named host's row while the stream is not connected`,
  `::shows a pending or failed sync of a named host's row, and the error of a failed one`,
  `::marks the trigger of All hosts while a listed host is not reachable or its sync failed`);
  the stream state and heartbeat of each named host
  (`ui/src/events/provider-hosts.test.tsx::keeps each named host's stream state and last heartbeat, and drops them with the stream`);
  a sync line waits for a host the app cannot reach
  (`ui/src/components/host-sync.test.tsx::reads a pending sync of a host the app cannot reach as waiting for host`);
  one stream per reachable host under its own keys, with the connection
  state `local`'s (`ui/src/events/provider-hosts.test.tsx::opens one stream per reachable host, and none for a host that is down`,
  `::dispatches a named host's event under that host's keys`,
  `::keeps the connection state about local while a named host's stream drops`);
  the dispatcher moves one host's keys
  (`ui/src/events/dispatch.test.ts::patches and invalidates the keys of the host the event came from, and no other`,
  `::refetches only the host whose stream had a gap`,
  `::patches the host's row on host_updated, and reads a host that came up again`,
  `::ignores host_updated from a named host's own stream`);
  a session event patches the sessions lists without refetching them
  (`ui/src/events/dispatch.test.ts::patches a moved session into every cached list and refetches none of them`,
  `::moves a session between status lists and leaves another goal's list alone`,
  `::adds a new session to the lists it belongs in`,
  `::refetches a list whose read is still under way, which a patch would lose to`);
  `?host=` (`ui/src/routes/paths.test.ts::writes ?host= beside a named host's panel param`,
  `::leaves ?host= out for local, and drops the one a named host's panel left`,
  `::resolves a link without ?host= to local`); `useHostLists`
  (`ui/src/api/host-lists.test.tsx::tags each host's rows with the host, and reports the host that failed on its own`,
  `::keeps the other hosts' rows when one host answers with an error`); the
  board, the panel, the dialog and the attention round trip
  (`ui/src/features/goals/hosts.test.tsx::merges the goals of every host under All, each goal naming its host`,
  `::reads only the picked host, and names no host on its goals`,
  `::opens a goal's panel on the host its link names`,
  `::defaults the goal dialog to the picked host, and reloads its lists when the host changes`,
  `::opens the goal dialog on local under All`,
  `::shows a question a remote host's stream delivers, and answers it on that host`).

- Catalog screens show a sync line per host, with skipped permissions on the
  Learned tab (`ui/src/features/permissions/permissions-hosts.test.tsx::shows a sync line per host: in sync, pending, or failed with its error`,
  `::counts the learned rows a host skipped on the Learned tab only`); the AI tab
  shows and writes each host's device
  (`::shows each host's own device and writes a device change to that host alone`);
  the repositories table names the hosts with and without a checkout
  (`ui/src/features/repositories/repositories-page.test.tsx::says which hosts hold a checkout of each repository and which lack one`,
  `::says a repository with no forge has none to match a checkout by, not that it has no checkout`).
- The agents screen groups an agent under one tab with a chip per host and
  writes to the chip's host
  (`ui/src/features/agents/agents-hosts.test.tsx::groups an agent of two hosts under one tab, with a chip per host, and writes to the chip's host`,
  `::reads only the picked host, with no chips`,
  `::says which host's agents failed to load beside the agents of the host that answered, with a retry`,
  `::names the failed host beside a host that answered with no agents`,
  `::names every host whose agents failed, each with a retry, when none answered`).
- A host the app cannot reach is named, with its reason, in each dialog, card
  and error state (rule 75): the goal dialog's default and disabled option
  (`ui/src/features/unreachable-hosts.test.tsx::opens the goal dialog on local when the picked host cannot be reached`,
  `::lists an unreachable host in the goal dialog as a disabled option with its reason`);
  the AI tab (`::shows the reason of an unreachable host on the AI tab, with no skeleton`);
  an error of a named host and the hint for `local` alone
  (`::shows an error state, with the host's reason and no URL hint, when a named host's goals fail`,
  `::shows the daemon hint only for local, on a table of a named host`);
  a warning for some hosts and an error state for all
  (`::warns, not errors, when only one of two hosts fails to list sessions`,
  `::shows an error state for each host when every host fails to list sessions`);
  the Agent filter (`::lists the agents of each selected host in the sessions Agent filter`).
- The Forge tabs merge hosts, open a panel on the row's host, offer the hosts of
  a repository for a review, and name the host of each issue
  (`ui/src/features/forge/forge-hosts.test.tsx::merges the pull requests of every host under All, and opens a panel on its row's host`,
  `::narrows to one repository by its forge name on every host, and skips a host without it`,
  `::offers the hosts where the repository is registered for Start review, the row's host first`,
  `::merges the issues of every host, and names the host of each`).
- The logs drawer opens the log of the picked host and of the host its select
  names (`ui/src/features/system/daemon-logs-drawer.test.tsx::opens the log of the picked host, and of local under All, and follows the drawer's own host select`,
  `::defaults to local under All hosts`,
  `::shows no host select while the daemon lists local alone`,
  `::lists a host it cannot reach as a disabled option with its reason, and defaults to local over such a pick`,
  `::keeps the host it opened on when that host goes down, and says why`).
- The dashboards screen queries the host its select names
  (`crates/ariadne-daemon/tests/it/dashboards.rs::every_named_series_of_the_shipped_dashboard_carries_the_host`,
  `ui/src/routes/dashboards.test.tsx::queries every host under All, the picked host otherwise, and the host the select names`,
  `::opens on the host the header picker names`,
  `::shows no host select with local alone`,
  `::lists a host it cannot reach as a disabled option with its reason, and opens on All over such a pick`,
  `::sends no stale host when the saved pick is no longer listed`,
  `::keeps a chosen host when it drops and returns, says why, and neither switches the iframe nor reloads it`,
  `::keeps the header's host when it drops and returns before a pick here, and says why`,
  `::follows the header pick only until a host is picked on the screen`); the iframe's datasources point at
  `/v1/hosts/{host}/` (`ui/dashboard/src/datasource-api.test.ts::points both datasources at the named host, or at all of them`,
  `ui/dashboard/src/page-params.test.ts::reads the daemon, the dashboard and the theme off the query string`).
- The goal dialog follows rule 79
  (`ui/src/features/goals/hosts.test.tsx::clears the model and the effort when the goal dialog's host changes`,
  `::warns in the goal dialog of a host whose sync is failed, and still creates`,
  `::warns in the goal dialog of a host whose sync is pending, and still creates`,
  `::names the host in the toast of a goal created on a named host, and offers all hosts when the pick leaves it out`).


Stepper acceptance for rule 22f:
- Body typography, Markdown, header, recommendation, progress, companion grouping and pick advancement:
  `ui/src/features/sessions/console/form-stepper.test.tsx::groups Claude companions and advances a single pick before sending multiple picks`.
- Custom answers and Skip omit unselected content:
  `ui/src/features/sessions/console/form-stepper.test.tsx::sends only custom text and skips without content`.
- Single-question message and immediate submission:
  `ui/src/features/sessions/console/form-stepper.test.tsx::uses the message for one question and sends its choice immediately`.
- Keyboard focus, arrows, numbered Other row and Back:
  `ui/src/features/sessions/console/form-stepper.test.tsx::moves between rows and returns to the previous question`.
- Composer keys stay outside the card shortcuts:
  `ui/src/features/sessions/console/form-card.test.tsx::keeps composer number and Enter keys outside the question shortcuts`.
- Dismissed cards open read-only:
  `ui/src/features/sessions/console/form-stepper.test.tsx::keeps a dismissed card to one status and opens its full form read-only`.
- Number validation and submission:
  `ui/src/features/sessions/console/form-stepper.test.tsx::validates a number step and keeps shortcuts inside the card`.
- Summary expansion and marked read-only picks:
  `ui/src/features/sessions/console/form-stepper.test.tsx::opens an answered summary read-only with its picks marked`.
- Attention-strip companions and recommendation:
  `ui/src/features/goals/attention-forms.test.tsx::answers one Claude question with its companion and Recommended badge in place`.

## Known gap

The goal panel, the session panel, the session detail view, the sessions
screen, the task card, the goal actions and the attention list
(`goal-panel.tsx`, `session-panel.tsx`, `session-detail-view.tsx`,
`sessions-page.tsx`, `task-card.tsx`, `goal-actions.tsx`, `attention.ts`)
have no `*.test.tsx`
or `*.test.ts` of their own: what they draw is exercised only incidentally,
through `detail-panels.test.tsx`, `pane-focus.test.tsx` and
`goal-board.test.tsx` mounting the screens that hold them. Rule 19's
account of the sessions screen in particular rests on reading
`sessions-page.tsx` rather than on a test asserting it, apart from the host
behaviour of rule 61.

The tests run in jsdom and measure no layout. No test proves that the board's
columns fit at 33.9rem, the floor of rule 39. The test of rule 39 reads the
classes that set the floor and the hide widths, not the drawn widths.

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`,
037 (`GET /v1/hosts` and `host_updated`),
`ui/dashboard/`, `ui/src/test/flaky.ts`,
`ui/dashboard/src/test/flaky.ts` (the explicit bounded test marker).

Interaction reference: [Introducing Devin](https://docs.devin.ai/get-started/devin-intro),
for conversation beside selected activity details. Ariadne retains its own components and themes.

The user’s later Develop instruction places permission requests in the details pane or drawer, like tools.
This overrides the original requirement to keep permission responses inline. Questions remain in the conversation.

### Loaded transcript find

The console offers Find in transcript and a scoped Ctrl/Cmd+F shortcut.
Text fields, editors, the composer, and nested dialogs retain keyboard ownership.
Search uses the current view's loaded block text, including folded activity and tool details.
Each occurrence has a count and an active mark in a separate result pane.
Previous and Next controls select occurrences. Enter and Shift+Enter in the search field also select occurrences.
The pane leaves transcript folds, reading position, pending requests, and drafts intact.
Escape closes find and restores focus. Stream updates retain the selected block and occurrence offset while available.
The coverage message identifies unloaded history. Load earlier history uses the existing page action.

Acceptance: `ui/src/features/sessions/console/transcript-find.test.tsx` proves result navigation, history selection stability, focus return, and shortcut ownership.
