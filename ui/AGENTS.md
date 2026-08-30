# ui/AGENTS.md

Conventions for changing the desktop app under `ui/`. Read this before editing
anything here; commit-message and history rules live in the root
[`AGENTS.md`](../AGENTS.md).

Before committing, run `npm test`, `npm run typecheck`, `npm run lint` and
`npm run check:unused`.

## Layout

```
src/
  api/             typed HTTP client, generated types, the query-key convention,
                   and the query building blocks the features share
  events/          the one SSE connection, its dispatcher, the reconnect machinery
  stores/          zustand: settings (daemon URL), stream status
  hooks/           shared hooks (connection state, global shortcuts, focus return)
  lib/             format, clipboard, keyboard chords
  routes/          the route table, URL/panel helpers, 404
  components/      app shell, sidebar, theme + settings + connection, and the
                   table / form dialog / delete dialog / panel pieces features reuse
    ui/            shadcn/ui primitives
  features/
    command-palette/ ⌘K: search over every entity, plus the actions
    goals/         the goals board (swimlanes, attention strip), the goal panel,
                   and the attention count the shell shows everywhere else
    tasks/         the task panel: facts, diff, reviews, history, conversation
    sessions/      the sessions screen, the session panel and the terminal
    profiles/      profiles screen and the prompts a profile overrides
    repositories/  the registered checkouts goals are created against
    agents/        agent-kind screen: the flags each CLI is launched with
    system/        the daemon-logs drawer and the log stream behind it
  test/            setup, render harness, DTO fixtures and the browser stand-ins
                   the suite shares
src-tauri/         the Tauri shell (deliberately empty: no commands)
```

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

## Conventions

### Query keys

Defined once in `src/api/query-keys.ts` and used through the `qk` helper — never
write a key literal. Every key is `[entity, "list" | "detail", ...]`:

```
["goals",        "list", filters]   ["goals",    "detail", id]
["tasks",        "list", filters]   ["tasks",    "detail", id]
["sessions",     "list", filters]   ["sessions", "detail", id]
["profiles",     "list", filters]   ["profiles", "detail", id]
["repositories", "list", filters]   ["repositories", "detail", id]
["agents",       "list", {}]        ["models",   "list", {}]
["agent-events", "list", filters]
```

Sub-resources hang off their detail key: `["tasks", "detail", id, "messages"]`,
`… "reviews"`, `… "transitions"`, `… "diff"`, `["goals", "detail", id,
"messages"]`, `["sessions", "detail", id, "logs"]`, `["profiles", "detail", id,
"prompts"]`. Two consequences the event dispatcher depends on: invalidating
`qk.tasks.lists()` refetches every task list without disturbing an open detail
view, and invalidating a detail key also invalidates that entity's
sub-resources.

`src/api/queries.ts` holds what the features would otherwise each spell out:
`cacheRow` / `dropRow` (write the detail entry, refetch the lists), `useRowAction`
(a confirmed action, optimistic where the landing status is knowable) and
`usePostMessage`.

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
| `goal_deleted` | remove `goals.detail`, invalidate `goals.lists` and every task and session key |
| `task_created` | patch `tasks.detail`, invalidate `tasks.lists` |
| `task_updated` | patch `tasks.detail`, invalidate `tasks.lists`, and `tasks.transitions` when the event carries a transition |
| `message_created` | invalidate `tasks.messages` or, for a goal-level message, `goals.messages` |
| `review_created` | invalidate `tasks.reviews` |
| `session_created`, `session_updated` | patch `sessions.detail`, invalidate `sessions.lists` |
| `agent_event` | invalidate `agentEvents.lists` |
| `profile_created`, `profile_updated` | patch `profiles.detail`, invalidate `profiles.lists` |
| `profile_deleted` | remove `profiles.detail`, invalidate `profiles.lists` |
| `repository_created` | patch `repositories.detail`, invalidate `repositories.lists` |
| `repository_updated` | the same, plus every goal key — goals carry their repositories inline |
| `repository_deleted` | remove `repositories.detail`, invalidate `repositories.lists` |

The daemon has **no replay**: anything that happened while the stream was down
is simply gone. So both a reconnect and the daemon's `resync` control event
(sent when this client fell too far behind, just before the daemon hangs up)
invalidate *everything*. Reconnection itself — capped exponential backoff with
jitter, closing the old socket before opening a new one — is
`src/events/reconnecting-stream.ts`, shared with the session-pane and
daemon-log streams; `DomainEventStream` adds the protocol and publishes its
state through `useStreamStore`.

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
only exits once the last stream client disconnects). The daemon therefore says
so itself: a `heartbeat` control event (`{version, started_at}`) on open and
every 15 idle seconds. That cadence arms the stream's **idle budget** — 2.5
beats, re-armed by every frame whatever it carried — and a longer silence
calls `forceReconnect`. It is the one timer the client keeps, and the only
thing standing between a dead daemon and a screen that looks fine; the Retry
button in the connection banner is the same call, made by hand.

The heartbeat is also who the UI is talking to: `useStreamStore` keeps what the
last one said, and that is where the footer's daemon version and uptime come
from — a changed `started_at` is a daemon that restarted.

`src/events/stream.ts` declares the event kinds as a total record over the
generated `DomainEventKind`, and `dispatchDomainEvent` ends in a `never`
exhaustiveness check — a new event kind in the daemon fails to compile in both
places until it is handled.

### Routes

Every route is in `src/routes/router.tsx`, and that is where a screen is added.
There is no per-feature route file: there are a handful of routes, half of them
one line, and a file that mounted one said less about its feature than the line
it held. What the header calls a screen rides on the route's own `handle`.

Five screens have URLs of their own — `#/goals`, `#/sessions`, `#/profiles`,
`#/agents`, `#/repositories` — and `#/` redirects onto the board. Goals, tasks
and sessions have no pages: their details open as **side panels** driven by
search params (`?goal=` on the board, `?task=` over any screen, `?session=` for
a session's own panel, `?tab=sessions&session=` for a session inside a goal's or
a task's panel), which `src/components/detail-panels.tsx` reads. The old
`#/goals/:goalId` and `#/tasks/:taskId` deep links survive as redirects onto the
board with the panel open.

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
built on them — `taskConversationFrom`, `sessionTerminalFrom`,
`taskSessionPanelFrom`, and `attentionTarget` above all — inherits the rule, so
the attention list answers correctly from every screen it is carried onto.

Link with the helpers in `src/routes/paths.ts` (`paths.goal`, `taskPanelFrom`,
`sessionPanelFrom`, `panelSessionTo`, …) rather than hand-written paths, so a
panel opened from a list keeps the screen and the filters behind it.

A **hash router** is used on purpose: in a packaged build the frontend is served
straight off Tauri's asset protocol with no history fallback, so a reload on a
deep link has to resolve client-side.

### Keyboard

Chords are bound once, by the shell, in `src/hooks/use-global-shortcuts.ts` —
`window`, bubble phase, skipped when the keystroke was already handled or is
going into a text field, an editor, or the textarea xterm reads a session's pane
through. The typed chords are skipped inside a dialog or a menu too, where a
bare letter belongs to whatever is on top. `Escape` is deliberately *not* bound:
it belongs to whatever is on top, and Base UI's dialogs already close the
topmost one, so a global handler would take two layers down at once.

`?` is the one typed chord `isBareKey` cannot guard, since Shift is how the
character is typed on most layouts: `matchesHelpKey` matches the character the
keyboard produced instead. It opens `src/components/keyboard-shortcuts-dialog.tsx`,
whose rows are `SHORTCUT_HELP` — built from the chords the shell binds, in this
table's order, so the sheet cannot fall behind them. "Keyboard shortcuts" in the
palette opens the same sheet.

The palette (`src/features/command-palette/`) leads with **Needs attention** —
the attention list's own rows (`features/goals/attention.ts`), which decide
where a pick lands through the same `attentionTarget` the strip and the alerts
ask, so a question opens the thread it was asked in and a prompt opens the pane
it is waiting in — and then the actions, including the ones that only
exist for what the screen underneath has open: a new task in the goal whose
panel is up, `ariadne attach <id>` for the task or session that is. It searches
the goal, task, session and profile lists that are **already in the query
cache** — the same keys their own screens read, fetched only while it is open —
and its rows navigate through `src/routes/paths.ts`, so a task stacks its panel
on whatever screen it was opened over. Two notes on the matching, both in
`score.ts`:

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

Add components with `npx shadcn@latest add <name>`. Three known snags: it may
write to a literal `@/` directory (move the files into `src/`), its output
occasionally trips `noUnusedLocals` or a Biome rule (fix the file, or add an
override under `src/components/ui/**` in `biome.json`), and `npm run
check:unused` fails on a primitive nothing renders — so use what you scaffold,
or delete it.

`shadcn` itself stays in `dependencies` rather than `devDependencies`:
`src/index.css` imports `shadcn/tailwind.css`, so it is a runtime dependency and
not only the scaffolding CLI.
