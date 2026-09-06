---
id: desktop-app
status: current
updated: 2026-09-06
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10]
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
   panel (facts, diff, messages, history), sessions and a terminal, skills,
   repositories, agent kinds and their launch flags, models and which of them
   may be staffed on, and a daemon-logs drawer.
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
9. On Linux the shell also tells GTK its own application id (`enableGTKAppId`,
   so the identifier `dev.ariadne.ui` becomes the window's), which is how a
   desktop shell matches the window on screen to the entry the installer
   wrote for it (016) — and therefore how the window gets the app's icon
   rather than a generic one.
10. The primary surface is the macOS Tauri window (WebKit): a layout change is
    verified there, not only in a browser.
11. The app is checked by `npm test`, `npm run typecheck`, `npm run lint` and
    `npm run check:unused` before a commit.

## Acceptance criteria

- 70 test files cover the features, the API layer and the event stream; each
  screen's behaviour is asserted in its own `*.test.tsx` beside it.
- The task's channel reads as one list, every kind is told apart, and both
  ends of a message are named by the skills they work with
  (`ui/src/features/tasks/task-messages.test.tsx`).
- Every judgement the orchestrator makes about a task can be made here too:
  how a task ends
  (`ui/src/features/tasks/task-form-dialog.test.tsx::sends how the task ends`),
  whether it is reviewed (`::takes every reviewer off`), and whether the goal
  is over (`ui/src/features/goals/goal-actions.test.tsx::completing a goal`).
- The models screen lists the catalog with a switch apiece, and says why
  where the daemon refuses one
  (`ui/src/features/models/models-page.test.tsx`) — the rule of 011 read from
  the surface that acts on it.
- The skills screen groups the shipped skills apart from the user's own
  (`ui/src/features/skills/skills-page.test.tsx`), and offers reset for the
  first and delete for the second and never the other way round
  (`ui/src/features/skills/skill-editor.test.tsx`) — the rule of 017 read from
  the client side.
- The repository dialog puts a placeholder refusal on the landing-briefing
  field rather than on the branch its message also names
  (`ui/src/features/repositories/repository-form-dialog.test.tsx`).
- The attention strip holds a placeholder while its lists load and survives a
  partial failure (`ui/src/features/goals/attention-strip.test.tsx`).
- Unused exports fail `npm run check:unused`.

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
