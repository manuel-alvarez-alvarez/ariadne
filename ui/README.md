# Ariadne Desktop

Desktop client for `ariadned`. A [Tauri 2](https://v2.tauri.app) window around a
Vite + React + TypeScript app.

The UI is a **pure REST/SSE client of the daemon's TCP listener** — exactly what
the CLI is, over HTTP instead of a unix socket. It never links against the
daemon crates and there are no Tauri commands: everything it shows comes from
`http://<tcp_listen>/v1/...` and the live event stream. That is what makes the
same code work in a browser tab and in the packaged app.

## Running it

The daemon must be listening on TCP, which is off by default. Add it to
`~/.ariadne/config.toml` and restart the daemon:

```toml
tcp_listen = "127.0.0.1:7676"
```

Then, from `ui/`:

```sh
npm install
npm run tauri dev      # desktop window (starts the Vite dev server itself)
npm run dev            # or just the web app, http://localhost:1420
npm run tauri build    # packaged app + installers under src-tauri/target/release/bundle
```

The daemon URL defaults to `http://127.0.0.1:7676` and is editable in the
settings dialog (the gear in the header). It is persisted to `localStorage`
under `ariadne.settings`; the theme lives under `ariadne.theme`. Changing the
URL clears the query cache and reconnects the event stream.

The sticky footer at the bottom of the window carries the connection state:
green when both `/v1/health` and the event stream are live, amber when REST
works but the stream is down (screens load but stop updating themselves), red
when the daemon is unreachable. Hover it for the URL, daemon version and
uptime; it is a button, and a coming task attaches the daemon-logs drawer to
the click.

### Scripts

| Script | What it does |
|---|---|
| `npm run dev` | Vite dev server on port 1420 (fixed — `tauri dev` points at it) |
| `npm run build` | typecheck + production bundle into `dist/` |
| `npm run typecheck` | `tsc -b`, no emit |
| `npm run test` | Vitest, once (`test:watch` to keep it running) |
| `npm run lint` | Biome lint + format check |
| `npm run lint:fix` | Biome, applying safe fixes |
| `npm run format` | Biome formatter only |
| `npm run gen:api` | regenerate the API types (below) |
| `npm run tauri <cmd>` | the Tauri CLI (`dev`, `build`, `info`, …) |

## Regenerating the API types

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
normalization happens in memory before generating: utoipa derives `operationId`
from the handler function name, so ids collide across tags (`goals::list` and
`tasks::list` are both `list`), and `scripts/gen-api.mjs` qualifies them with
their tag — `goals_list`, `tasks_list` — which is what the generated
`operations` map is keyed by. The `paths` types, which is what the client uses,
are unaffected.

## Layout

```
src/
  api/             typed HTTP client, generated types, query-key convention
  events/          the SSE connection and its dispatcher
  stores/          zustand: settings (daemon URL), stream status
  hooks/           shared hooks (connection state, global shortcuts)
  routes/          router, URL helpers, 404
  components/      app shell, sidebar, theme + settings + status
    ui/            shadcn/ui primitives
  features/
    command-palette/ ⌘K: search over every entity, plus the actions
    goals/         goals list + goal detail (incl. its task board)
    tasks/         task detail
    sessions/      session list + session detail, embedded by the panels
    profiles/      profiles screen
    agents/        agent-kind screen: the flags each CLI is launched with
src-tauri/         the Tauri shell (deliberately empty: no commands)
```

`ui/src-tauri` is **excluded from the root cargo workspace** (see `exclude` in
the repository's `Cargo.toml`), so `cargo build --workspace` never builds the
desktop shell and the UI's dependency tree stays out of `Cargo.lock`.

### Calling the daemon

```ts
import { api, ApiError, qk, unwrap } from "@/api"

const goals = useQuery({
  queryKey: qk.goals.list({ limit: 50 }),
  queryFn: () => unwrap(api().GET("/v1/goals", { params: { query: { limit: 50 } } })),
})
```

Paths, query parameters, bodies and responses are all typed from the generated
schema. `unwrap` returns the response body and throws an `ApiError` carrying the
daemon's `{error: {code, message, details}}` envelope — branch on `error.code`
(`task_not_found`, `illegal_transition`, …) rather than parsing messages.
`error.isNetworkError` is the "never reached the daemon" case.

## Conventions

### Query keys

Defined once in `src/api/query-keys.ts` and used through the `qk` helper — never
write a key literal. Every key is `[entity, "list" | "detail", ...]`:

```
["goals",    "list",   filters]        ["goals",    "detail", id]
["tasks",    "list",   filters]        ["tasks",    "detail", id]
["sessions", "list",   filters]        ["sessions", "detail", id]
["profiles", "list",   filters]        ["profiles", "detail", id]
```

Sub-resources hang off their detail key: `["tasks", "detail", id, "messages"]`,
`… "reviews"`, `… "transitions"`, `… "diff"`, `["goals", "detail", id,
"messages"]`, `["sessions", "detail", id, "logs"]`. Two consequences the event
dispatcher depends on: invalidating `qk.tasks.lists()` refetches every task list
without disturbing an open detail view, and invalidating a detail key also
invalidates that entity's sub-resources.

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
| `goal_created`, `goal_updated` | patch `goals.detail`, invalidate `goals.lists` |
| `task_created` | patch `tasks.detail`, invalidate `tasks.lists` |
| `task_updated` | patch `tasks.detail`, invalidate `tasks.lists`, and `tasks.transitions` when the event carries a transition |
| `message_created` | invalidate `tasks.messages` or, for a goal-level message, `goals.messages` |
| `review_created` | invalidate `tasks.reviews` |
| `session_created`, `session_updated` | patch `sessions.detail`, invalidate `sessions.lists` |
| `agent_event` | invalidate `agentEvents.lists` |
| `profile_created`, `profile_updated` | patch `profiles.detail`, invalidate `profiles.lists` |
| `profile_deleted` | remove `profiles.detail`, invalidate `profiles.lists` |

The daemon has **no replay**: anything that happened while the stream was down
is simply gone. So both a reconnect and the daemon's `resync` control event
(sent when this client fell too far behind, just before the daemon hangs up)
invalidate *everything*. `DomainEventStream` handles reconnection itself with
capped exponential backoff and jitter, and publishes its state through
`useStreamStore`.

"Reconnect" here means *any open that follows a gap*, not just an open that
follows a previous one. A first connection that only came up after a few failed
attempts — the app launched before the daemon did — is a reconnect too: REST
queries may have loaded during those seconds, and whatever the daemon published
in between is unrecoverable. Only a first connection that succeeded straight
away skips the invalidation. `src/events/stream.test.ts` pins both directions.

An `EventSource` alone is **not** enough to notice a daemon that went away: the
socket can stay in `OPEN` with no `error` ever firing, and the UI would go
quietly stale. With `ariadned` that is the normal case, not an edge case — its
graceful shutdown waits for in-flight requests and an SSE stream never
finishes, so the connection outlives the daemon that is stopping (the daemon
only exits once the last stream client disconnects). The REST health probe is
therefore also the stream's watchdog: losing it calls `forceReconnect`, getting
it back calls `reconnectIfClosed` instead of waiting out the backoff. That is
why `healthQueryOptions` is shared rather than restated per call site.

`DOMAIN_EVENT_KINDS` in `src/events/stream.ts` is a total record over the
generated `DomainEventKind`, and `dispatchDomainEvent` ends in a `never`
exhaustiveness check — a new event kind in the daemon fails to compile in both
places until it is handled.

### Routes

A feature with screens of its own owns a `routes.tsx` exporting its
`RouteObject[]`, which `src/routes/router.tsx` mounts under the shell. Add
screens there, not in the router. Link with the helpers in
`src/routes/paths.ts` rather than hand-written paths.

Not every feature has routes. Sessions has none: a session is opened from the
Sessions tab of the goal or the task panel, which the `?tab=`/`?session=`
search params drive (`panelSessionTo` in `src/routes/paths.ts` builds a link to
one). `#/sessions` is not a URL the app answers — it falls through to the 404.

A **hash router** is used on purpose: in a packaged build the frontend is served
straight off Tauri's asset protocol with no history fallback, so a reload on a
deep link has to resolve client-side.

### Keyboard

| Chord | What it does |
|---|---|
| `⌘K` / `Ctrl+K` | the command palette |
| `⌘,` / `Ctrl+,` | settings |
| `Escape` | closes the palette, then the topmost panel |

Both chords answer to **either** modifier, on every platform: the app runs in a
Tauri WebView and in a browser tab, and a chord that silently does nothing
because the platform was sniffed wrong is worse than one that answers to both.
Only the hint printed next to the header's search button picks a side
(`shortcutLabel` in `src/lib/shortcuts.ts`).

They are bound once, by the shell, in `src/hooks/use-global-shortcuts.ts` —
`window`, bubble phase, skipped when the keystroke was already handled or is
going into a text field, a CodeMirror editor or the textarea xterm reads a
session's pane through. `Escape` is deliberately *not* bound: it belongs to
whatever is on top, and Base UI's dialogs already close the topmost one, so a
global handler would take two layers down at once.

The palette (`src/features/command-palette/`) searches the goal, task, session
and profile lists that are **already in the query cache** — the same keys their
own screens read, fetched only while it is open — and its rows navigate through
`src/routes/paths.ts`, so a task stacks its panel on whatever screen it was
opened over. Two notes on the matching, both in `score.ts`:

- ulids live in an entry's `keywords`, matched literally, never fuzzily: 26
  characters of random letters answer to almost any subsequence query, so
  leaving them in the scored text let `planner` find a task called "Keyboard
  support";
- cmdk sorts the rows *inside* a group and leaves the groups where they were
  written, so the palette orders the groups itself, by their best match.

### UI components

shadcn/ui in the `base-nova` style, which is built on
[Base UI](https://base-ui.com) rather than Radix — composition uses the `render`
prop, not `asChild`. Note that this style ships `field` (`Field`, `FieldLabel`,
`FieldError`, …) instead of the older `form` wrapper; `react-hook-form` and
`zod` are installed to go with it.

Add components with `npx shadcn@latest add <name>`. Two known snags: it may
write to a literal `@/` directory (move the files into `src/`), and its output
occasionally trips `noUnusedLocals` or a Biome rule — fix the file, or add an
override under `src/components/ui/**` in `biome.json`.

## Rules for the screen tasks

The four feature tasks (goals, tasks, sessions, profiles) run **in parallel**.
Sessions is the exception to "screen": it owns no route of its own — its list
and detail views are embedded by the goal and task panels' Sessions tab.
Each one owns exactly one directory under `src/features/` and must stay inside
it:

- **Do not edit `package.json`.** Everything the screens need is already
  installed: CodeMirror 6 (including `@codemirror/merge` and
  `@uiw/react-codemirror`), `@xterm/xterm` with the fit and web-links addons,
  `react-markdown` + `remark-gfm`, and the shadcn/ui primitives listed in
  `src/components/ui/`. If something is genuinely missing, say so in the task
  thread rather than adding it.
- **Do not edit the shared files**: `src/api/**`, `src/events/**`,
  `src/stores/**`, `src/lib/**`, `src/routes/router.tsx`,
  `src/components/app-shell.tsx`, `src/components/app-sidebar.tsx`, the theme,
  settings and connection components, or `biome.json` / the tsconfigs. Adding a
  new shadcn primitive under `src/components/ui/` is fine.
- **Add routes in your feature's own `routes.tsx`**, which the router already
  mounts.
- `src/components/stub-screen.tsx` is scaffolding: drop the import when you
  replace your stub. The file goes away with the last usage.
