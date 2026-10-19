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
npm run check:audit            # dependency advisories, always whole
```

`ui/dashboard/` is a separate npm package (see
[The dashboard package](#the-dashboard-package)) with its own copy of every
command above, run from inside it rather than from `ui/`.

For a confirmed intermittent test, import `flaky` from `src/test/flaky.ts` in
this package, or from the same path in `ui/dashboard/`. In a test beside that
helper, use this form:

```ts
import { expect } from "vitest"
import { flaky } from "./flaky"

flaky("reports the result", () => {
  expect(2 + 2).toBe(4)
})
```

Adjust the relative import for another directory. The marker retries twice,
then fails under its original test name. Follow the evidence rule in the root
[`AGENTS.md`](../AGENTS.md#intermittent-tests) before adding one.

The pre-commit hook (`.githooks/pre-commit`) runs, for a staged file under
`ui/` (outside `ui/src-tauri` and `ui/dashboard`): `npx biome check <staged
files>` and `npm run typecheck`, both from inside `ui/` (`npm ci` first when
`ui/node_modules` is missing). For a staged file under `ui/src-tauri`: `cargo
fmt --all -- --check` from inside `ui/src-tauri`. For a staged file under
`ui/dashboard`: the same two commands, from inside `ui/dashboard` (`npm ci`
first when `ui/dashboard/node_modules` is missing).

The pre-push hook (`.githooks/pre-push`) runs the whole suite but the test
suite, for a push that touches `ui/` (outside `ui/dashboard`): `npm run
check:audit`, `npm run lint`, `npm run typecheck` and `npm run check:unused`,
each in `ui/` (`npm ci` first when `ui/node_modules` is missing). `npm test`
takes about a minute, so the hook leaves it to CI; run it by hand before a
push when the change warrants it. For a push that touches `ui/src-tauri`:
`cargo fmt --all -- --check` and `cargo check`, both from inside
`ui/src-tauri`. For a push that touches `ui/dashboard`: the same four `npm
run` commands as `ui/`'s own, from inside `ui/dashboard` instead.

### Dependency advisories

The bar for `ui/` is `npm run check:audit` exiting 0, not the bare `npm
audit --audit-level=high` — that still exits 1 on the one allowlisted
advisory below, by design, so run the wrapper, never the bare command, to
judge whether a change is clean.

`npm run check:audit` (`scripts/check-audit.mjs`) wraps `npm audit
--audit-level=high`, which reads `package-lock.json` alone, against the
GitHub Advisory Database; nothing here raises or lowers that threshold.
`overrides` in `package.json` forces a transitive dependency past an
advisory a direct dependency alone cannot clear, each pinned to the lowest
release that does, so a direct dependency's own declared range still picks
the newest version its author intended wherever that is enough on its own:

| Override | Why |
| --- | --- |
| `@modelcontextprotocol/sdk` | pulled in by `shadcn`'s own MCP server; GHSA-6qxp-vccf-f47h |
| `brace-expansion` | pulled in at two different majors, by `shadcn` and by `shadcn`'s own `ts-morph`; GHSA-q2hr-2g5m-vwhr, GHSA-qhr7-859c-m2p7, GHSA-6j4f-fj2g-mc7p |
| `fast-uri` | pulled in by `@redocly/openapi-core`, itself pulled in by `openapi-typescript`; six GHSA ids, all fixed in 3.1.8 |
| `js-yaml` | pulled in by the same `@redocly/openapi-core`; GHSA-2883-xcg3-v3hh |
| `proxy-addr` | pulled in by `shadcn`'s own MCP server; GHSA-jqcg-44mw-7w3h |
| `source-map-js` | pulled in by `tailwindcss`; GHSA-68fv-2mgg-jv7q |
| `undici` | pulled in by `shadcn`'s own MCP server, and again by `jsdom`; six GHSA ids, all fixed in 8.12.2 |

One high finding has no override: `braces` (GHSA-vfj7-8cjw-p6xm, a
stack-exhaustion denial of service), reached through `shadcn`'s own
`ts-morph` and `fast-glob`. Every release of `braces` up to and including its
latest, `3.0.3`, is listed vulnerable — there is no fixed version to pin an
override to — and `npm audit`'s own suggested fix is downgrading `shadcn` to
`1.0.0`, three majors back, before it grew the codemod machinery that pulls
`ts-morph` and `fast-glob` in at all. That trade was not taken: `braces` is
exercised only by a human running `npx shadcn@latest add <name>` against a
trusted local checkout, never by the shipped app, so the realistic exposure
is far below the advisory's own severity.

Plain `npm audit` has no per-advisory ignore list the way `.cargo/audit.toml`
does for Rust, so `scripts/check-audit.mjs` is one: it runs the bare command,
and where that fails, parses its JSON report and passes anyway only when
every high or critical finding in it carries an advisory id in its
`ALLOWLIST` — a high finding on any other id, anywhere in the tree, still
fails the same way the bare command would. Each `ALLOWLIST` entry carries
the advisory id, the package, the reason, and the date it was added:

```js
{
  id: "GHSA-vfj7-8cjw-p6xm",
  package: "braces",
  reason: "a stack-exhaustion denial of service in the shadcn codemod " +
    "chain (ts-morph, fast-glob), reached only by a developer's own " +
    "local `npx shadcn add` command, never by the shipped app; every " +
    "published braces release is vulnerable, so there is no version " +
    "to pin an override to",
  added: "2026-10-16",
}
```

`scripts/check-audit.test.mjs` drives that decision directly, against a
fixed, made-up report, so it needs no live advisory data and does not change
behavior if the real one ever does. Revisit (narrow or drop the entry) when
`braces` ships a fix, or when `shadcn` drops the dependency.

`ui/src-tauri` is excluded from the root cargo workspace (see
[Layout](#layout)), so [`crates/AGENTS.md`](../crates/AGENTS.md)'s cargo
commands never reach it. One root-level step does reach it: `cargo machete
.`, run from the repository root, reads source and descends into
`ui/src-tauri` too.

See the root [`AGENTS.md`](../AGENTS.md#checks) for installing the hooks and
running one by hand.

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
- Fail a context, hook or type a test needs that is only reachable by importing
  the large component that defines it; give it its own module (`page-header.tsx`
  reaches `AppShell`'s `PageHeaderContext` through `page-header-context.ts`, not
  `app-shell.tsx` itself) so a test that needs the value is not charged for
  importing the component tree around it.
- Fail a `userEvent` interaction that simulates typing or clicking for a step a
  test's claim does not cover — clearing a field before picking a value, say;
  use `fireEvent` there and keep `userEvent` for the interaction under test.

Find where a slow file's time goes — module import, environment, or the tests
themselves — in the `Duration` line `npx vitest run <file>` already prints
(`transform`, `setup`, `import`, `tests`, `environment`), then break the
import time down further with `npx vitest run --experimental.importDurations.print
--experimental.importDurations.limit 20 <file>`, which prints each imported
module's own and cumulative time.

## Layout

```
src/
  api/             typed HTTP client, generated types, the query-key convention,
                   and the query building blocks the features share
  events/          the one SSE connection, its dispatcher, the reconnect machinery
  telemetry/       OTLP logs and metrics emitted by the desktop app
  stores/          zustand: settings (daemon URL), stream status
  hooks/           shared hooks (connection state, global shortcuts, focus return)
  lib/             format, clipboard, keyboard chords, design tokens
  routes/          the route table, URL/panel helpers, the Dashboards page, panel
                   history, and the error and 404 pages
  components/      app shell, sidebar, theme + settings + connection, and the
                   table / form dialog / delete dialog / panel pieces features reuse
    ui/            shadcn/ui primitives
  features/
    command-palette/ ⌘K: search over every entity, plus the actions
    goals/         the goals board (swimlanes, attention strip), the goal panel,
                   and the attention count the shell shows everywhere else
    tasks/         the task panel: facts, steps, diff, messages, sessions, history
    dashboard/     the Dashboards screen's data layer: the dashboard picker's list,
                   and the reset mutation — the iframe itself is `ui/dashboard/`
                   (see "The dashboard package")
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

Write an external link as a plain `<a href>`. The app-root handler
(`src/hooks/use-open-external-links.ts`) opens it through the opener plugin.
Do not use `window.open` or rely on `target="_blank"` alone: the Tauri
webview ignores it.

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
["dashboards",   "list", {}]       ["dashboards", "detail", name]
```

`workflows.parse` and `pullRequests.search` are the two keys outside the
`list`/`detail` convention: each caches one derived read — a parsed document,
a repository's matching requests — that is neither a list nor a detail.

`dashboards` is keyed by name rather than id, the same as `skills` and
`workflows`: `DashboardDto` carries no id of its own.

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
| `goal_updated` | patch `goals.detail`, invalidate `goals.lists` |
| `goal_deleted` | remove `goals.detail`, invalidate `goals.lists`, every task and session key, and `attention.lists` |
| `task_created` | patch `tasks.detail`, invalidate `tasks.lists` |
| `task_updated` | patch `tasks.detail`, invalidate `tasks.lists` and `attention.lists`; a transition on the event also invalidates `tasks.transitions` and `tasks.diff` — a transition can be the task landing, and the diff answers for the merge commit once there is one |
| `task_branch_updated` | invalidate `tasks.diff` for the task — a commit in the author's worktree, with nothing about the task row itself changed |
| `message_sent` | invalidate `tasks.messages` for the task it is about; a message about the goal itself belongs to no task's channel |
| `session_created` | patch `sessions.detail`, invalidate `sessions.lists` and `outsideSessions.lists` — a resume adopts an outside row; a session carrying a `pull_request_id` also invalidates that request's `pullRequests.detail` and `pullRequests.lists` |
| `session_updated` | patch `sessions.detail`, invalidate `sessions.lists` and `attention.lists`; a session carrying a `pull_request_id` also invalidates that request's `pullRequests.detail` and `pullRequests.lists` |
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
| `dashboard_created`, `dashboard_updated` | patch `dashboards.detail`, invalidate `dashboards.lists` |
| `dashboard_deleted` | remove `dashboards.detail`, invalidate `dashboards.lists` |

`dashboard_deleted` is the one `*_deleted` event that is fat like its own
`*_created`/`*_updated`, carrying the whole `DashboardDto` rather than the
generic `DeletedDto` every other one does — a dashboard has no id of its own
for `DeletedDto.id` to carry, only `name`, which the whole DTO already has.

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
`#/workflows`, `#/agents`, `#/permissions`, `#/repositories`, `#/dashboards`,
and the Forge screen's two tabs, `#/forge/pull-requests` and `#/forge/issues`
(`#/forge` opens the first) — and `#/` redirects onto the board. `#/models`
redirects onto `#/agents`, which folded the model catalog in; `#/tasks`
redirects the same way onto the board. The Dashboards screen is
`src/routes/dashboards.tsx`: a picker and a Reset action over `GET /v1/dashboards`
(`src/features/dashboard/queries.ts`), and an iframe over the `ui/dashboard`
package's own build (or, in development, its own dev server — see "The
dashboard package"), which renders the picked Perses dashboard.
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

## The dashboard package

`ui/dashboard/` is the Perses dashboard the Dashboards screen opens in an iframe
(`src/routes/dashboards.tsx`) — its own npm package, on React 18 and
`@tanstack/react-query` v4, because [`@perses-dev/dashboards`](https://www.npmjs.com/package/@perses-dev/dashboards)
and its plugins peer-depend on those, a major and a line behind what `ui/`
itself runs. It shares nothing with `ui/`'s own `src/api` — two endpoints are
not worth a second copy of that codegen pipeline — so its calls to the daemon
are hand-rolled (`src/dashboard-client.ts`).

```
dashboard/
  src/
    main.tsx            mounts Page
    page.tsx            the provider tree (query client, MUI theme, Perses
                         charts theme, snackbar host, query-param provider,
                         plugin registry) and ViewDashboard itself
    page-params.ts       reads `daemon`, `dashboard` and `theme` off this
                         page's own query string
    use-host-messages.ts listens for the host's `postMessage`s
    datasource-api.ts    the app's one `DatasourceApi`: a global Prometheus
                         and a global Loki datasource, `directUrl` the daemon
    dashboard-client.ts   `GET`/`PUT /v1/dashboards/{name}`
    plugins.ts            the plugin loader: every datasource, panel and
                         variable kind the task names, through
                         `dynamicImportPluginLoader`
    test/setup.ts          its own daemon-fetch stub, parallel to `ui/`'s
  patches/                `patch-package` fix for a packaging bug (below)
  scripts/                its own copies of `check-unused.mjs` and
                         `check-audit.mjs`, scoped to this tree
```

**Build and dev wiring.** `vite.config.ts` sets `base: "/dashboard/"` and
builds into `../dist/dashboard` — under the app's own `dist/`, so the
packaged webview serves it from the same origin as everything else. `npm run
build` in `ui/` builds the app first and this package second (`npm --prefix
dashboard run build`): building this package first would be wiped by Vite's
own `emptyOutDir` on the app's default `dist`, which contains it. In
development, `ui/vite.config.ts`'s dev server proxies `/dashboard` to this
package's own dev server (port 1421, `ui/`'s own is 1420), so the iframe is
same-origin in both the packaged app and `npm run dev` alike, matching rule 1
of the task that added this package.

**The message protocol**, between `src/routes/dashboards.tsx` (the host) and this
package's `page.tsx` — see its own docstring for the fuller rationale:

- the iframe opens on `?daemon=<url>&dashboard=<name>&theme=<light|dark>`;
- the host posts `{type: "theme", theme}` on every theme change after the
  first paint — the first is already on the query string;
- the host posts `{type: "reload"}` when the shown dashboard's own
  `updated_at` moves under it without the iframe's own `src` changing — an
  edit saved from another window or the CLI, landing here as a
  `dashboard_updated` event the dispatcher already patched into the row; the
  package re-reads `GET /v1/dashboards/{name}` and re-renders rather than
  reloading the document in the literal sense.

**The peer dependency override.** None needed for any `@perses-dev/*`
package itself: every plugin's `peerDependencies` (checked against the
published packages, not assumed) resolves cleanly against `dashboards`,
`plugin-system`, `components` and `client` all at `^0.54.0` and `spec` at
`^0.2.0`. Two *patches* were needed instead, not an override:
`@perses-dev/logs-table-plugin@0.3.0`'s `LogRow.js` imports
`./ansiColors.css`, a file missing from the published tarball (fixed
upstream, but only in an 0.4.0 prerelease that would drag every other
`@perses-dev/*` package here to its own 0.55.0-rc line). `patches/` adds the
missing file back — the standard 16-colour ANSI palette `ansi_up` (this
plugin's own dependency) defines its class names from — as a
`postinstall: patch-package` script reapplies on every `npm ci`. Drop it
once a stable 0.4.0 ships.

`@perses-dev/components@0.54.0`'s `formatValue` — the one function every
panel kind (stat, gauge, pie, bar, table, time series) formats its numbers
through — stringifies a `NaN` query result as the literal `"NaN"`, straight
off `Intl.NumberFormat`. Every panel's number, including one a user wrote
their own dashboard's query to produce, must show Perses's own "No data"
state instead (036). The second patch makes `formatValue` answer `"No data"`
for a `NaN` value before it reaches a unit formatter, so this holds for
every dashboard the app opens, not only the shipped one.
`src/format-value.test.ts` proves it through a deep import
(`@perses-dev/components/dist/model/units`): the package's own barrel pulls
in its MUI theme, which `require()`s raw `.css` files that only Vite's build
step, never a plain Vitest module load, knows how to read.

**Checks.** The same five commands as `ui/`'s own
(`npx biome check`/`npm run typecheck`/`npm test`/`npm run check:unused`/`npm
run check:audit`), run from inside `ui/dashboard` rather than `ui/` — see
"On a task branch" above, the pre-commit and pre-push hooks, and `ci.yml`'s
`dashboard` job. `check:unused`'s `PEER_ONLY` set is this package's own
`TOOLING`-shaped list for packages nothing here imports by name because a
Perses package peer-depends on them instead; a missing peer is an
install-time error, not a reachable import.

A second `biome.json` next to an existing one, each the default `root: true`,
is a configuration error the moment anything scans across the boundary — not
a lint finding, a refusal to run at all — so `ui/biome.json`'s own
`files.includes` carries `"!dashboard"` to stay out of this package's tree
entirely, the same way it already stays out of the generated API files; a
plain `biome check .` run from inside `ui/dashboard` itself needs nothing
extra; the (unfixed, Biome 2.2–2.5) bug behind writing that exclusion as
`"!dashboard/**"` instead — silently matching nothing, so the boundary is
never actually drawn — is why it is spelled without the trailing `/**`.

Tests cover the glue — reading the page's own query string, the host-message
listener, the datasource lookup, and the load/save calls, each with a
stubbed `fetch` — and `page.tsx`'s own lifecycle with `ViewDashboard` itself
mocked to a component that only records its own mount count and the document
it was given (`page.test.tsx`): real `ViewDashboard` is Perses's own
integration surface, not this package's, and far too heavy for jsdom, but
two of this package's own bugs lived in exactly how `page.tsx` drives it —
`mounted.current` never came back up after React's StrictMode development
remount, silently dropping every later `load()`, and a `{type: "reload"}`
message updated `dashboardResource` as a prop into the same `ViewDashboard`
instance, which only ever reads it once, as its internal store's initial
value, and so never picked up the fresh document. `key={version}`, bumped on
every successful load, is what makes a reload remount the component instead.
