# assets/AGENTS.md

Conventions for changing branding and the demo recordings. Commit-message and
history rules live in the root [`AGENTS.md`](../AGENTS.md).

## What is here

- `branding/` — `logo.svg` and `banner.svg`, hand-authored.
- `demo.gif` and `demo-ui.gif` — the two demos in the root `README.md`'s
  hero table, side by side: the CLI creating a goal, and the desktop app's
  own form doing the same thing. Both show the same goal, "Add rate limiting
  to the login endpoint", with the same two tasks, in the same small login
  HTTP service, and both open the orchestrator's console on the same
  scripted exchange — one codebase and one plan, told through two
  interfaces.
- `demo-orchestrator-agent.py` — the stub ACP agent both recordings register
  and run as the orchestrator, in python3 with only the standard library,
  built on the shape of the test stub at
  `crates/ariadne-daemon/tests/it/common/acp.rs`. It plays one fixed,
  two-turn conversation, regardless of which recording started it or what
  either prompt actually says: the first `session/prompt` — the daemon's own
  initial prompt, sent as soon as the orchestrator session starts — reads a
  file of the demo repository and asks whether the rate limit should apply
  per IP address or per account; the second — the answer a person types into
  the console, forwarded as the next `session/prompt` — accepts it. Neither
  recording's two tasks come from this agent; a real orchestrator would
  propose them over the same turn this plays short, but `seed-demo-goal.py`
  (below) still adds them and finalizes the plan over the REST API instead,
  exactly as it already did before this agent existed. Each recording
  registers it as a custom `[[acp_agents]]` entry of its own throwaway
  `config.toml`, under the id `demo-orchestrator`; the daemon probes every
  configured agent before it starts serving requests, so by the time a
  recording's daemon is up, `demo-orchestrator:demo-model` is already a
  valid `--model`.
- `seed-demo-repo.sh` — the login HTTP service both recordings seed into
  their scratch repository (a `package.json` and two source files), so the
  two GIFs show the same codebase: `bash assets/seed-demo-repo.sh <repo>`.
  The caller has already run `git init` there; this script writes the files
  and commits them.
- `seed-demo-goal.py` — adds the same two tasks to the goal each recording
  creates through its own user-facing path (the CLI types `ariadne goal
  create`; the UI fills the New goal form), then finalizes the plan the way
  a real orchestrator would once the user agreed to it, over the daemon's
  REST API: `python3 assets/seed-demo-goal.py <base-url> <model>`. It finds
  the goal by its title and adds the tasks and finalizes the moment it does,
  with no wait of its own for the orchestrator's console exchange — each
  recording is what holds it back until the stub has accepted the typed
  answer (see `demo.tape` and `record-demo-ui.mjs` below), since starting it
  any earlier would let the tasks and the finalized plan show up before the
  answer did. `<model>` is only ever the model the two added tasks' columns
  run on, never the orchestrator's own — that one is
  `demo-orchestrator-agent.py` in both recordings, pinned through `goal
  create`'s own `--model` or the New goal form's model picker, not through
  this script. No `jq` or other non-stdlib dependency: this repo's own
  tooling already assumes `python3` (`crates/AGENTS.md`), never a bare `jq`.
- `demo.tape`, rendered with [vhs](https://github.com/charmbracelet/vhs), and
  [`record-demo-cli.sh`](record-demo-cli.sh), the script that owns it:
  `bash assets/record-demo-cli.sh`. Never call `vhs` on the tape directly —
  `demo.tape` only types commands, it does not set up or tear down what they
  need. The script builds the release binary, creates a throwaway daemon on
  its own `ARIADNE_HOME` (`/tmp/ariadne-demo-home`, never the real
  `~/.ariadne`) with a loopback `tcp_listen` port and `demo-orchestrator-agent.py`
  registered as its orchestrator agent, seeds its scratch repository with
  `seed-demo-repo.sh`, runs `vhs` over the tape, and removes the daemon and
  the scratch directories when the recording ends or the script is
  interrupted — the same lifecycle `record-demo-ui.mjs` owns for its own
  recording, below. After the goal is created, the tape attaches to its
  orchestrator's console (`ariadne goal attach`), types the answer to the
  stub's question, and detaches with two `Ctrl-C`, the same way a person
  would — only then, back at the shell, does a hidden step start
  `seed-demo-goal.py` in the background, redirected: a `Hide`/`Show` pair
  only skips *capturing* frames, it does not clear the terminal's own
  scrollback, so a command typed and run between them would still show once
  `Show` resumes, which is what the `clear` at the end of each hidden block
  is for.
- `record-demo-ui.mjs`: `node assets/record-demo-ui.mjs`. It builds the
  release binaries, creates an isolated Ariadne home with a two-second inert
  `cli_bin` (the scheduler can create metadata rows for the demo, but it
  cannot start a real agent process) and `demo-orchestrator-agent.py`
  registered as its orchestrator agent, seeds its own scratch repository with
  `seed-demo-repo.sh`, fills the New goal form in the UI with the same goal
  `demo.tape` types — typed at a visible speed rather than filled in one
  frame — and the orchestrator pinned to that same stub, opens its console
  from the goal panel's Sessions tab and types the same answer `demo.tape`
  types (the UI's console is the CLI's own console, the same bytes, in a
  terminal emulator — see `ui/src/features/sessions/session-terminal.tsx`),
  then runs `seed-demo-goal.py` the same way to add the same tasks and
  finalize, and opens one of them. It drives the UI dev server with
  Playwright, at twice the device pixel density so the GIF's downscaled text
  stays sharp, and removes the daemon, the dev server and the scratch
  directory itself when the recording ends or the script is interrupted.

## Rules

- Never edit a GIF by hand; it is rendered output. Change the tape, the
  recording script, the stub agent, or the two shared seeding scripts, then
  regenerate both — a title, task or conversation changed in one and not the
  other makes the two demos disagree.
- Regenerate the GIF whose recording shows a change you just made to the CLI
  or the UI it drives — a stale GIF documents a screen that no longer exists.
- Never record against the real `~/.ariadne`: both recordings build their own
  throwaway `ARIADNE_HOME` and tear it down after, so a recording run never
  touches, and never depends on, your own daemon's data.
- `demo-ui.gif` is rendered at 12 fps and about 1280px wide, and kept at 10MB
  or less; do not drop below either the frame rate or the width to make the
  size. If a change cannot keep all three, stop and report the size and the
  trade-off rather than silently shipping a blurrier or choppier GIF.
