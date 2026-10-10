# Ariadne Desktop

Desktop client for `ariadned`. A [Tauri 2](https://v2.tauri.app) window around a
Vite + React + TypeScript app. The goals board draws a stepped task in its
workflow's column, and the Workflows screen edits the catalog — shipped and
your own, each with a document editor and a live kanban preview.

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

The sidebar's last child carries the connection state, and it has exactly one
source: the event stream. Green while it is open and the daemon is beating,
amber while the first connection is being made, red once it is gone — which is
the same thing as the screens no longer being live. Hover it for the URL, the
daemon version and its uptime, both of which come from the `heartbeat` the
stream carries; clicking it opens the daemon-logs drawer. Nothing polls: an
idle window makes no requests at all.

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
| `npm run check:unused` | fails on unused declared dependencies, production exports, or non-test source files |
| `npm run gen:api` | regenerate the API types (see below) |
| `npm run tauri <cmd>` | the Tauri CLI (`dev`, `build`, `info`, …) |

Regenerating the API types after a daemon change is covered in
[`AGENTS.md`](AGENTS.md#regenerating-the-api-types).

## Keyboard

| Chord | What it does |
|---|---|
| `⌘K` / `Ctrl+K` | the command palette |
| `⌘,` / `Ctrl+,` | settings |
| `N` | new goal, from any screen |
| `[` | fold the sidebar down to an icon rail, and back |
| `G` then `G`/`S`/`K`/`A`/`R` | goals, sessions, skills, agents, repositories |
| `?` | the cheat sheet: this table, in the app |
| `Escape` | closes the palette, then the topmost panel |
| `⌘Esc` / `Ctrl+Esc` | leaves a focused console for the pane or the modal around it |

The two ⌘ chords answer to **either** modifier, on every platform: the app runs
in a Tauri WebView and in a browser tab, and a chord that silently does nothing
because the platform was sniffed wrong is worse than one that answers to both.
Only the hint printed next to the header's search button picks a side
(`shortcutLabel` in `src/lib/shortcuts.ts`).

See [`AGENTS.md`](AGENTS.md#keyboard) for how chords are bound and guarded, and
for the command palette.
