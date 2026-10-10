# ui/AGENTS.md

Conventions for changing the desktop app under `ui/`. Read this before editing
anything here; commit-message and history rules live in the root
[`AGENTS.md`](../AGENTS.md).

On a task branch, run the checks of what you changed, and only those:

```sh
npx vitest run <path>          # the test files of the change
npx biome check <paths>        # the files of the change
npm run typecheck              # the project's types, always whole
npm run check:unused           # dependency, export and source reachability
```

Before a commit on `main`, run the whole suite: `npm test`, `npm run
typecheck`, `npm run lint` and `npm run check:unused` — `npm run check` runs
all four at once.

Touching `ui/src-tauri`: CI runs `cargo fmt --all -- --check` and
`cargo check`, both from inside `ui/src-tauri` — it is excluded from the root
cargo workspace (see [Layout](#layout)), so [`crates/AGENTS.md`](../crates/AGENTS.md)'s
cargo commands never reach it. One root-level step does reach it: `cargo
machete .`, run from the repository root, reads source and descends into
`ui/src-tauri` too.

The suite needs no daemon and no agent. The daemon is a stubbed `fetch`, a
stubbed `EventSource` and a stubbed `WebSocket` (`src/test/`), and a session's
console is the real xterm.js on that socket stand-in, fed the bytes a test
writes — jsdom lays nothing out, so the grid keeps xterm's default 80 by 24
and what it drew is read back off its rows. Do not put a mock of that weight in
`src/test/setup.ts`: a mock there slows every file, and the dialog tests then
time out.

## Review UI tests

- Fail `setTimeout`, real timers, or sleeps; use fake timers only for timers under test and advance them by hand.
- Keep Testing Library and `userEvent` timers real.
- Fail an added or raised `waitFor` or `findBy*` timeout without a comment that names what is slow and why.
- Fail a test that passes only on a second run; repeat `npx vitest run <file>` under the root bounded busy loop.
- Fail a changed test above one second or a changed file above ten seconds; make it faster.
- Fail a mock in `src/test/setup.ts` or a global stub a test leaves behind; use local mocks and `vi.stubGlobal`.
- Fail a test that depends on another test's render or state; render and reset its own state.

## Layout

```
src/
  api/             typed HTTP client, generated types, the query-key convention,
                   and the query building blocks the features share
  events/          the one SSE connection, its dispatcher, the reconnect machinery
  stores/          zustand: settings (daemon URL), stream status
  hooks/           shared hooks (connection state, global shortcuts, focus return)
  lib/             format, clipboard, keyboard chords, design tokens
  routes/          the route table, URL/panel helpers, the Stats page, panel
                   history, and the error and 404 pages
  components/      app shell, sidebar, theme + settings + connection, and the
                   table / form dialog / delete dialog / panel pieces features reuse
    stats/         each stat family's section, and the tiles / table / chart
                   pieces they share
    ui/            shadcn/ui primitives
  features/
    command-palette/ ⌘K: search over every entity, plus the actions
    goals/         the goals board (swimlanes, attention strip), the goal panel,
                   and the attention count the shell shows everywhere else
    tasks/         the task panel: facts, steps, diff, messages, sessions, history
    forge/         the Forge screen: one sidebar entry, its two tabs the
                   pull requests and issues routes below it
    pull-requests/ the Forge tab of the open pull requests — every one, the
                   user's own, or the ones that ask for their review — and the
                   pane over one
    issues/        the Forge tab of open issues from an enabled repository's
                   forge, and filling a goal dialog from one
    sessions/      the sessions screen, the session panel and its console: a
                   terminal pane on the daemon's terminal socket
    models/        the pin picker, the model catalog and the agent summary
    skills/        skills screen: the catalog, and the document each one is
    workflows/     the Workflows screen: the catalog, the document editor, and
                   its parsed kanban preview
    repositories/  the registered checkouts goals are created against, their
                   forge integration, and the webhook tunnel switch
                   the settings dialog shows
    agents/        agents screen: the flags each registry agent is launched
                   with, and the model catalog each may be staffed on (what
                   `#/models` redirects onto)
    permissions/   the Permissions screen: Learned (every approval a `learn`
                   or `ai` repository has kept, and one added by hand) and AI
                   (the model's settings behind the `ai` permission mode)
    system/        the daemon-logs drawer and the log stream behind it
  test/            setup, render harness, DTO fixtures and the browser stand-ins
                   the suite shares
src-tauri/         the Tauri shell: no commands, only the per-OS webview setup
                   it needs before it opens (`lib.rs`)
```

Every screen renders under one header bar (`components/app-shell.tsx`), not a
header of its own: the bar's `h1` is the route's `handle.title`, and a
screen's `PageHeader` (`components/page-header.tsx`) carries no heading at
all. Its `actions` portal into that bar through `PageHeaderContext`, and its
`description` becomes the tooltip of the header's title; mounted with no shell
around it — every feature test mounts its screen on its own — it renders
`actions` in place instead. See spec 015 ("Behavior") for the full rule.

`ui/src-tauri` is **excluded from the root cargo workspace** (see `exclude` in
the repository's `Cargo.toml`), so `cargo build --workspace` never builds the
desktop shell and the UI's dependency tree stays out of `Cargo.lock`.

### Calling the daemon

```ts
import { api, qk, unwrap } from "@/api"

const tasks = useQuery({
  queryKey: qk.tasks.list({ goal: goalId }),
  queryFn: () => unwrap(api().GET("/v1/tasks", { params: { query: { goal: goalId } } })),
})
```

Paths, query parameters, bodies and responses are all typed from the generated
schema. `unwrap` returns the response body and throws an `ApiError` carrying the
daemon's `{error: {code, message, details}}` envelope — branch on `error.code`
(`task_not_found`, `illegal_transition`, …) rather than parsing messages.
`error.isNetworkError` is the "never reached the daemon" case.

`src/api/types.ts` holds only the schema aliases the app actually reads: it is
not a complete mapping of the document, and `npm run check:unused` fails on an
alias nothing imports. Add one when a screen needs it.

### Regenerating the API types

`src/api/schema.d.ts` is generated from the daemon's OpenAPI document by
[openapi-typescript](https://openapi-ts.dev). **Both it and the `openapi.json`
snapshot it was generated from are committed**, so nothing here needs a running
daemon to build. Regenerate whenever the daemon's API changes:

```sh
npm run gen:api                            # live daemon on 127.0.0.1:7676
npm run gen:api -- http://host:7676        # live daemon elsewhere
npm run gen:api -- ../some-spec-dump.json  # a spec dump on disk
```

and commit both files. `openapi.json` is the daemon's verbatim document; one
normalization happens on the script's own copy before generating: utoipa derives `operationId`
from the handler function name, so ids collide across tags (`goals::list` and
`tasks::list` are both `list`), and `scripts/gen-api.mjs` qualifies them with
their tag — `goals_list`, `tasks_list` — which is what the generated
`operations` map is keyed by. The `paths` types, which is what the client uses,
are unaffected.

## Conventions

### Query keys

Defined once in `src/api/query-keys.ts` and used through the `qk` helper — never
write a key literal. Every key is `[entity, "list" | "detail", ...]`:

```
["goals",        "list", filters]   ["goals",    "detail", id]
["tasks",        "list", filters]   ["tasks",    "detail", id]
["tasks",        "detail", id, "messages" | "transitions" | "diff"]
["sessions",     "list", filters]   ["sessions", "detail", id]
["outside-sessions", "list", filters]
["skills",       "list", filters]   ["skills",   "detail", name]
["workflows",    "list", {}]        ["workflows", "detail", name]
["workflows",    "parse", document]
["repositories", "list", filters]   ["repositories", "detail", id]
["agents",       "list", {}]        ["models",   "list", {}]
["attention",    "list", {}]
["acp-agents",   "list", {}]
["agent-events", "list", filters]
["permissions",  "detail", "ai"]  ["forge",    "detail", "tunnel"]
["issues",       "list", repository, { assigned }]
["pull-requests", "list", filters]  ["pull-requests", "detail", id | "repository:number"]
["pull-requests", "search", { repo, q }]
["learned-permissions", "list", filters] ["learned-permissions", "detail", id]
["stats",        "list", family, filter]
```

`workflows.parse` and `pullRequests.search` are the two keys outside the
`list`/`detail` convention: each caches one derived read — a parsed document,
a repository's matching requests — that is neither a list nor a detail.

`stats` is one group over the five stat families (`work`, `time`, `spend`,
`models`, `attention`), `[stats, "list", family, { since, repo }]`, read
through `qk.stats.<family>(filter)`: a task or a session that moves can be a
fact of any family, so the dispatcher invalidates `qk.stats.all()` whole.

`permissions.ai()` and `forge.tunnel()` are the keys with no list beside
them: each is one settings row (`GET /v1/permissions/ai`,
`GET /v1/forge/tunnel`), not a collection.

Several keys have no `detail` beside their list: `outsideSessions`, `agents`,
`models`, `acpAgents`, `agentEvents` and `issues` are each read whole or
filtered, with nothing the app opens on one id alone.

The outside-sessions list is the one key the daemon pages: its cursor stays
out of the key, because the pages of one filter are the pages of one infinite
query.

Sub-resources hang off their detail key: `["tasks", "detail", id, "messages"]`,
`… "transitions"`, `… "diff"`. Two consequences the event dispatcher
depends on: invalidating `qk.tasks.lists()` refetches every task list without
disturbing an open detail view, and invalidating a detail key also invalidates
that entity's sub-resources.

`src/api/queries.ts` holds what the features would otherwise each spell out:
`cacheRow` / `dropRow` (write the detail entry, refetch the lists) and
`useRowAction` (a confirmed action, optimistic where the landing status is
knowable).

### The event stream

`GET /v1/events/stream` is opened **once** for the whole app, by
`EventStreamProvider`. Screens must not open their own `EventSource`: they read
the query cache and it stays live.

`src/events/dispatch.ts` is the only place events meet the cache. Events are fat
— each carries the full updated DTO — so for every kind the rule is the same:

- **patch the detail** with `setQueryData(qk.<entity>.detail(id), dto)`, so open
  detail screens update with no round trip;
- **invalidate the lists** with `qk.<entity>.lists()`, because list responses are
  filtered and paginated and cannot be patched blind.

| event | effect |
|---|---|
| `goal_created` | patch `goals.detail`, invalidate `goals.lists` |
| `goal_updated` | patch `goals.detail`, invalidate `goals.lists` and `stats.all` |
| `goal_deleted` | remove `goals.detail`, invalidate `goals.lists`, every task and session key, and `attention.lists` |
| `task_created` | patch `tasks.detail`, invalidate `tasks.lists` |
| `task_updated` | patch `tasks.detail`, invalidate `tasks.lists`, `stats.all` and `attention.lists`; a transition on the event also invalidates `tasks.transitions` and `tasks.diff` — a transition can be the task landing, and the diff answers for the merge commit once there is one |
| `task_branch_updated` | invalidate `tasks.diff` for the task — a commit in the author's worktree, with nothing about the task row itself changed |
| `message_sent` | invalidate `tasks.messages` for the task it is about; a message about the goal itself belongs to no task's channel |
| `session_created` | patch `sessions.detail`, invalidate `sessions.lists` and `outsideSessions.lists` — a resume adopts an outside row; a session carrying a `pull_request_id` also invalidates that request's `pullRequests.detail` and `pullRequests.lists` |
| `session_updated` | patch `sessions.detail`, invalidate `sessions.lists`, `stats.all` and `attention.lists`; a session carrying a `pull_request_id` also invalidates that request's `pullRequests.detail` and `pullRequests.lists` |
| `agent_event` | invalidate `agentEvents.lists` |
| `skill_created`, `skill_updated` | patch `skills.detail`, invalidate `skills.lists` |
| `skill_deleted` | remove `skills.detail`, invalidate `skills.lists` |
| `workflow_created`, `workflow_updated` | patch `workflows.detail`, invalidate `workflows.lists` |
| `workflow_deleted` | remove `workflows.detail`, invalidate `workflows.lists` |
| `repository_created` | patch `repositories.detail`, invalidate `repositories.lists` |
| `repository_updated` | the same, plus every goal key — goals carry their repositories inline — and `attention.lists`, a forge fetch error being a configuration recovery item's own evidence |
| `repository_deleted` | remove `repositories.detail`, invalidate `repositories.lists` and `attention.lists` |
| `ai_permissions_updated` | patch `permissions.ai()` whole — the one settings row, no list beside it |
| `forge_settings_updated` | patch `forge.tunnel()` whole — the tunnel switch and state, no list beside it |
| `pull_requests_changed` | invalidate every `pullRequests` key — requests are read live off the forge, so the event carries none |
| `issues_changed` | invalidate `issues.ofRepository(id)` — both assignment filters of the repository whose open issues moved; issues are read off the forge, so the event carries none |
| `learned_permission_created`, `learned_permission_updated` | patch `learnedPermissions.detail`, invalidate `learnedPermissions.lists` |
| `learned_permission_deleted` | remove `learnedPermissions.detail`, invalidate `learnedPermissions.lists` |

The daemon has **no replay**: anything that happened while the stream was down
is gone. So a reconnect — any open that follows a gap, a first connection that
only came up after failed attempts included — and the daemon's `resync`
control event (sent just before it hangs up on a client that fell behind)
invalidate *everything* (`src/events/stream.test.ts` pins both). Reconnection
itself — capped exponential backoff with jitter — is
`src/events/reconnecting-stream.ts`, shared with the daemon-log stream and, on
the same backoff, a session's console socket; `DomainEventStream` adds the
protocol and publishes its state through `useStreamStore`.

An `EventSource` can sit `OPEN` with no `error` while the daemon is gone, so
the stream keeps the one timer the client has: an **idle budget**, re-armed by
every frame including the daemon's own `heartbeat` (sent on open and every 15
idle seconds), that calls `forceReconnect` after 2.5 missed beats. The Retry
button in the connection banner makes the same call by hand.

The heartbeat is also who the UI is talking to: `useStreamStore` keeps what the
last one said, and that is where the footer's daemon version and uptime come
from — a changed `started_at` is a daemon that restarted.

`src/events/stream.ts` declares the event kinds as a total record over the
generated `DomainEventKind`, and `dispatchDomainEvent` ends in a `never`
exhaustiveness check — a new event kind in the daemon fails to compile in both
places until it is handled.

### The session console

A session's console is the CLI's console, the same bytes, in a terminal
emulator: `src/features/sessions/session-terminal.tsx` is an xterm.js
terminal (`@xterm/xterm`, fitted to its box by `@xterm/addon-fit`) on
`GET /v1/sessions/{id}/console/terminal`, the WebSocket over which the daemon
draws the console it runs itself (008). The pieces:

- `terminal-socket.ts` dials the socket and retries a drop on the event
  stream's backoff. The message shapes are `ariadne-api`'s
  `TerminalClientMessage` and `TerminalServerMessage`, written out by hand: a
  WebSocket has no body for the OpenAPI document to name them in, so the
  generated schema carries only the path. A close the daemon meant — the
  session ended, Ctrl-C twice, Ctrl-D — ends the console and is not retried;
  the pane offers Reopen instead.
- `terminal-keys.ts` maps a DOM key event onto the protocol's `key` message:
  crossterm's code and modifiers. xterm.js's own key handling is bypassed, so
  what a key means is decided once, on the daemon, for the CLI and the pane
  alike. A command-key chord and Ctrl+Shift+C / Ctrl+Shift+V are left to the
  browser (copy, and the paste event the pane sends as a `paste`); Option on
  macOS reads the letter off the physical key, so Alt-B is a word left there
  too.
- `terminal-theme.ts` reads the pane's colours and font off the app's tokens
  in `index.css`, in both themes: the six named colours map onto the status
  ramp, and `oklch()` is converted to hex, since xterm.js parses it only
  through a canvas.

The pane sends its size first — the daemon draws nothing before it — and on
every refit, and resets the emulator on every open, since each connection
draws the console whole. `src/test/web-socket.ts` is the stand-in the tests
drive it through.

### Routes

Every route is in `src/routes/router.tsx`, and that is where a screen is added.
There is no per-feature route file: there are a handful of routes, half of them
one line, and a file that mounted one said less about its feature than the line
it held. What the header calls a screen rides on the route's own `handle`.

Screens with URLs of their own — `#/goals`, `#/sessions`, `#/skills`,
`#/workflows`, `#/agents`, `#/permissions`, `#/repositories`, `#/stats`, and
the Forge screen's two tabs, `#/forge/pull-requests` and `#/forge/issues`
(`#/forge` opens the first) — and `#/` redirects onto the board. `#/models`
redirects onto `#/agents`, which folded the model catalog in; `#/tasks`
redirects the same way onto the board. The Stats screen is
`src/routes/stats.tsx`, and each stat family is a section of it,
`src/components/stats/<family>-section.tsx`, drawn through the shared
`StatSection`, `StatTiles`, `StatTable`, `StatTimeChart` and `StatBarChart`
beside it.
Goals, tasks and sessions have no pages: their details occupy one **floating pane** driven by
search params (`?goal=` on the board, `?task=` over any screen, `?session=` for
a session's own panel, `?tab=sessions&session=` for a session inside a goal's or
a task's panel, `?pr=` for a pull request's panel, `?skill=` and `?workflow=`
for the selected row on their own screens, `?focus=` for a control a link asks
the panel it opens to hand the keyboard to), which `src/components/detail-panels.tsx`
reads. The old `#/goals/:goalId` and `#/tasks/:taskId` deep links survive as
redirects onto the board with the panel open.

The pane floats over the screen at the window's right edge, so `<main>` keeps
its full width and layout behind it; the shell mounts `DetailPanels` outside
its row, and a scrim's click closes it the same as the close button does. See
spec 015 ("Behavior") for the exact width range, defaults and motion a change
here must keep. `PanelSheet` owns close decisions and focus return, over
`ui/docked-pane.tsx`, and opens a panel on its first control after the close
button. The pane holds one panel at a time: a task opened from a goal
replaces the goal's panel rather than stacking on it, carrying a breadcrumb
back that reopens the goal in its place, and session drill-downs replace the
panel outright, header and tabs included. Escape within the pane closes it
outright — never back to a goal a task replaced — and does nothing on the
board; modal portals own their own Escape. Keep `ui/sheet.tsx` for modal
drawers and the learned-permission detail.

**The sessions screen is the one exception**, and the only place a param means
two things: there `?goal=` and `?task=` are what the *list* is narrowed to — the
daemon's own filters on `GET /v1/sessions`, shown as a chip above the table —
so `#/sessions?goal=<id>` is every agent that has run for one goal rather than
a redirect to that goal's panel. Nothing but the session panel opens over that
screen, and the Context column links to those filters instead of to a panel;
the work itself is one step further on, from the session panel's own Goal and
Task links. See `src/features/sessions/filters.ts`.

That exception is why the helpers that open a panel take the screen they are
opened **from**: `taskPanelFrom(pathname, …)` lands on the board from the
sessions screen, where `?task=` would otherwise narrow the list instead of
opening what was picked, and `sessionPanelFrom` leaves that screen's `?goal=`
and `?task=` alone where every other screen has them cleared away. Everything
built on them — `sessionTerminalFrom`, `taskSessionPanelFrom`, and
`attentionTarget` above all — inherits the rule, so the attention list answers
correctly from every screen it is carried onto.

Link with the helpers in `src/routes/paths.ts` (`paths.goal`, `taskPanelFrom`,
`sessionPanelFrom`, `usePanelSessionTo`, `usePanelSessionNavigation`, …) rather
than hand-written paths, so a panel opened from a list keeps the screen and
the filters behind it.

A **hash router** is used on purpose: in a packaged build the frontend is served
straight off Tauri's asset protocol with no history fallback, so a reload on a
deep link has to resolve client-side.

### Keyboard

Chords are bound once, by the shell, in `src/hooks/use-global-shortcuts.ts` —
`window`, bubble phase, skipped when the keystroke was already handled or is
going into a text field, an editor, or a session's terminal (xterm.js types
through a hidden textarea). The typed
chords are skipped inside a dialog or a menu too, where a bare letter belongs
to whatever is on top. `Escape` is deliberately *not* bound:
it belongs to whatever is on top. `PanelSheet` handles it inside the docked
pane, and Base UI handles it inside dialogs. A global handler would close
two layers at once.

`?` is the one typed chord `isBareKey` cannot guard, since Shift is how the
character is typed on most layouts: `matchesHelpKey` matches the character the
keyboard produced instead. It opens `src/components/keyboard-shortcuts-dialog.tsx`,
whose rows are `SHORTCUT_HELP` — built from `SCREEN_SHORTCUTS` and the other
chords the shell binds, in the order it declares them, so the sheet cannot
fall behind them. `ui/README.md`'s keyboard table is kept by hand: match it to
`SHORTCUT_HELP`, row for row, in the same order, when a chord changes.
"Keyboard shortcuts" in the palette opens the same sheet.

The palette (`src/features/command-palette/`) leads with **Needs attention** —
the attention list's own rows (`features/goals/attention.ts`), which decide
where a pick lands through the same `attentionTarget` the strip and the alerts
ask, so a prompt opens the console it is waiting in and anything else opens
wherever it is otherwise read — and then the actions, including the ones that only
exist for what the screen underneath has open: a new task in the goal whose
panel is up, `ariadne attach <id>` for the task or session that is. It searches
the goal, task, session and skill lists that are **already in the query
cache** — the same keys their own screens read, fetched only while it is open —
and its rows navigate through `src/routes/paths.ts`, so a task opens on
whatever screen it was opened over, replacing a goal already open there. Two
notes on the matching, both in `score.ts`:

- ulids live in an entry's `keywords`, matched literally, never fuzzily: 26
  characters of random letters answer to almost any subsequence query, so
  leaving them in the scored text let `orchestrator` find a task called
  "Keyboard support";
- cmdk sorts the rows *inside* a group and leaves the groups where they were
  written, so the palette orders the groups itself, by their best match.

### UI components

shadcn/ui in the `base-nova` style, which is built on
[Base UI](https://base-ui.com) rather than Radix — composition uses the `render`
prop, not `asChild`. Note that this style ships `field` (`Field`, `FieldLabel`,
`FieldError`, …) instead of the older `form` wrapper; `react-hook-form` and
`zod` are installed to go with it.

Add components with `npx shadcn@latest add <name>`. Three known snags: it may
write to a literal `@/` directory (move the files into `src/`), its output
occasionally trips `noUnusedLocals` or a Biome rule (fix the file, or add an
override under `src/components/ui/**` in `biome.json`), and `npm run
check:unused` fails on a primitive nothing renders — so use what you scaffold,
or delete it.

`shadcn` itself stays in `dependencies` rather than `devDependencies`:
`src/index.css` imports `shadcn/tailwind.css`, so it is a runtime dependency and
not only the scaffolding CLI.
