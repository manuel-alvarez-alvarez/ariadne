# Ariadne Desktop

Desktop client for `ariadned`. A [Tauri 2](https://v2.tauri.app) window around a
Vite + React + TypeScript app.

The UI is a **pure REST/SSE client of the daemon's TCP listener** — exactly what
the CLI is, over HTTP instead of a unix socket. It never links against the
daemon crates and there are no Tauri commands: everything it shows comes from
`http://<tcp_listen>/v1/...` and the live event stream. That is what makes the
same code work in a browser tab and in the packaged app.

Changing code here? See [`AGENTS.md`](AGENTS.md) for the layout and engineering
conventions.

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
settings dialog (the gear in the header, or `⌘,`). It is persisted to
`localStorage` under `ariadne.settings`; the theme lives under `ariadne.theme`.
Changing the URL clears the query cache and reconnects the event stream.

The sticky footer at the bottom of the window carries the connection state, and
it has exactly one source: the event stream. Green while it is open and the
daemon is beating, amber while the first connection is being made, red once it
is gone — which is the same thing as the screens no longer being live. Hover it
for the URL, the daemon version and its uptime, both of which come from the
`heartbeat` the stream carries; clicking it opens the daemon-logs drawer.
Nothing polls: an idle window makes no requests at all.

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
| `npm run check:unused` | fails on a declared dependency nothing imports, or an export no other file imports |
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

## Keyboard

| Chord | What it does |
|---|---|
| `⌘K` / `Ctrl+K` | the command palette |
| `⌘,` / `Ctrl+,` | settings |
| `N` | new goal, from any screen |
| `[` | fold the sidebar down to an icon rail, and back |
| `G` then `G`/`S`/`P`/`A`/`R` | goals, sessions, profiles, agents, repositories |
| `?` | the cheat sheet: this table, in the app |
| `Escape` | closes the palette, then the topmost panel |

The two ⌘ chords answer to **either** modifier, on every platform: the app runs
in a Tauri WebView and in a browser tab, and a chord that silently does nothing
because the platform was sniffed wrong is worse than one that answers to both.
Only the hint printed next to the header's search button picks a side
(`shortcutLabel` in `src/lib/shortcuts.ts`).

See [`AGENTS.md`](AGENTS.md#keyboard) for how chords are bound and guarded, and
for the command palette.
