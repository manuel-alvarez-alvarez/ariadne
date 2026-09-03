---
id: desktop-app
status: current
updated: 2026-09-04
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44]
tests:
  - ui/src/features/**/*.test.tsx
  - ui/src/api/**/*.test.ts
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
2. The shell is a sidebar and a main area; a panel opens beside a list rather
   than replacing it, and the URL carries which panel is open.
3. Screens: the goals board (swimlanes plus an attention strip), the task
   panel (facts, diff, reviews, history), sessions and a terminal, profiles,
   repositories, agent kinds and their launch flags, and a daemon-logs drawer.
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
8. The primary surface is the macOS Tauri window (WebKit): a layout change is
   verified there, not only in a browser.
9. The app is checked by `npm test`, `npm run typecheck`, `npm run lint` and
   `npm run check:unused` before a commit.

## Acceptance criteria

- 68 test files cover the features, the API layer and the event stream; each
  screen's behaviour is asserted in its own `*.test.tsx` beside it.
- A profile screen shows one configurable system prompt and requests no
  lifecycle prompt (`ui/src/features/profiles/profile-editor.test.tsx`) — the
  ownership rule of 006 read from the client side.
- The repository dialog puts a placeholder refusal on the landing-briefing
  field rather than on the branch its message also names
  (`ui/src/features/repositories/repository-form-dialog.test.tsx`).
- The attention strip holds a placeholder while its lists load and survives a
  partial failure (`ui/src/features/goals/attention-strip.test.tsx`).
- Unused exports fail `npm run check:unused`.

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
