---
id: desktop-app
status: current
updated: 2026-09-11
areas: [ui]
commits: [f37dfd7b, 31bb7611, 10908591, b150ce44, 03f9c8b7, 29e6d84e, 1b09ac10, ced9f4f8, c11241f3]
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
   panel (facts, diff, messages, history), sessions with a terminal for a
   tmux one and a console for an `acp` one, outside sessions that a ready
   task can adopt as its author, skills, repositories, one repository's
   memory (list, search, delete), agent kinds and their launch flags, models
   and which of them may be staffed on, and a daemon-logs drawer.
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
12. A goal, task author and task reviewer each name a concrete
    `<agent_kind>:<model>` before their form can submit. An empty effort stays
    valid and uses that model's default effort (011).
13. The model picker lists concrete catalog entries only. No screen shows an
    automatic or default model; `auto` is an effort choice only.
14. The task form's skill boxes suggest only skills that can staff a task
    agent; the orchestrator's own playbook is not among them, for the author
    or a reviewer. The skills screen marks that playbook beside its built-in
    mark, staying editable and resettable like any other shipped skill.
15. The agent activity feed shows each event's one-line summary from the
    daemon; its raw payload stays available under the row.
16. The outside-sessions view lists the CLI (or, for an ACP session, its
    registry agent id), working directory, last activity and first prompt
    from `GET /v1/outside-sessions`, then adopts a matching session as the
    author of a ready task through `POST /v1/tasks/{id}/author-session`. Below
    the table it names every ACP agent from `GET /v1/acp-agents` that cannot
    list its sessions, with why.
17. On a task staffed with several authors (004) the task panel shows every
    one of them — its skills, its model, its own branch, and its status in the
    pick: the votes it has so far, or "Picked" once it is the one that won —
    and the reviewer pick itself: which author each reviewer chose. A task
    with one author shows the singular Author fact and no pick, unchanged.
18. An `acp` session's own tab renders its console rather than a terminal:
    the transcript from `GET /console` and `.../console/stream` (008), an
    input line posting to `.../console/input`, and permission questions
    answered inline. A message chunk, a thought, a tool call, a plan or
    anything else the runtime reports gets a readable row of its own where
    the console recognizes the kind, and the same one-line-summary-plus-
    payload row the agent activity feed already gives every event otherwise —
    the console carries no fixed list either. A message just sent is shown at
    once, pending, until the event that confirms it arrives; the `?session=`
    panel, the fullscreen dialog and the `?focus=` keyboard hand-off all work
    the same as they do for a tmux session's terminal.

## Acceptance criteria

- 75 test files cover the features, the API layer and the event stream; each
  screen's behaviour is asserted in its own `*.test.tsx` beside it.
- A task staffed with several authors shows each one's branch and its own
  vote count, marks the one the reviewers picked, and lists what each
  reviewer chose; a one-author task renders as before
  (`ui/src/features/tasks/task-panel.test.tsx::shows every author's own
  branch, marking only the one the reviewers picked`,
  `::shows an author's own vote count before the pick settles`,
  `::lists what each reviewer picked, oldest first`,
  `::keeps the singular Author fact and shows no pick on a one-author task`)
  — parity with `ariadne task inspect`'s own author and picks lines (004).
- One repository's memory lists, searches through the daemon's own search
  endpoint, and deletes an entry
  (`ui/src/features/memory/memory-page.test.tsx`) — parity with `ariadne
  memory ls|search|delete` (019).
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
- The orchestrator's own playbook is marked beside the built-in mark
  (`ui/src/features/skills/skill-editor.test.tsx::marks the orchestrator's own
  playbook as not a task staffing choice`) and left out of the task form's
  skill suggestions, for the author and every reviewer
  (`ui/src/features/tasks/task-form-dialog.test.tsx::suggests no
  orchestrator-only skill for the author or a reviewer`).
- The repository dialog puts a placeholder refusal on the landing-briefing
  field rather than on the branch its message also names
  (`ui/src/features/repositories/repository-form-dialog.test.tsx`).
- The attention strip holds a placeholder while its lists load and survives a
  partial failure (`ui/src/features/goals/attention-strip.test.tsx`).
- Unused exports fail `npm run check:unused`.
- The goal dialog and task dialog refuse a missing model, and the picker lists
  only concrete model ids
  (`ui/src/features/goals/create-goal-dialog.test.tsx`,
  `ui/src/features/tasks/task-form-dialog.test.tsx`,
  `ui/src/features/models/pin-picker.test.tsx`).
- The agent activity feed shows the daemon's summary and opens and closes the
  raw payload under its row
  (`ui/src/features/sessions/session-activity.test.tsx`).
- The outside-sessions view lists each discovered session, an ACP one named by
  its registry agent id, adopts one as the author of a matching ready task,
  and shows why an ACP agent without the session-listing capability offers
  none (`ui/src/features/sessions/outside-sessions-page.test.tsx`).
- An `acp` session's console renders a transcript from its stream, sends
  typed text to its input endpoint and shows it pending until confirmed, and
  answers an inline permission question the same way
  (`ui/src/features/sessions/acp-console.test.tsx`).

## Sources

`ui/AGENTS.md` (the layout and the conventions), `ui/src/`, `ui/src-tauri/`.
