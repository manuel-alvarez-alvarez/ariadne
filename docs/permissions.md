# Permission modes

Choose how Ariadne handles an ACP agent's tool-permission questions. The mode
belongs to a repository, and every agent that works in it follows it. The
default is `auto`, which keeps routine work moving. Choose `ask` when you want
to decide each request, or `learn` when repeated approved requests in one
repository should stop interrupting you. Choose `ai` to have the AI permission
model, which runs on your own machine, answer each request — see [The AI
permission model](#the-ai-permission-model) below.

## Set a repository's mode

Pass `--permission-mode` when you register a repository, or change it later:

```sh
ariadne repo add ~/projects/api --permission-mode learn
ariadne repo update <repo-id> --permission-mode ask
```

In the desktop app, open **Repositories**, then register or edit a repository
and choose **Permission requests**. The repositories table shows each
repository's mode, and so does `ariadne repo ls`.

The accepted values are `auto`, `ask`, `learn`, and `ai`. A repository
registered without one uses `auto`. The change applies to the next agent
launched in the repository; an agent already running keeps the mode it started
with. `ai` is accepted only once the AI permission model is enabled; a repository set to it
before that is refused, and the refusal says to enable the model first.

Which repository applies:

- A task's author and reviewers use the task's repository.
- A goal's orchestrator uses the goal's first repository.
- A resumed outside conversation uses the registered repository its directory
  is in. Outside every registered repository, it uses `auto`.

The mode cannot be set per task or in `~/.ariadne/config.toml`. A
`permission_mode` key left in `config.toml` stops the daemon from starting, so
remove it.

## What each mode does

| Mode | Behaviour | Use it when |
| --- | --- | --- |
| `auto` | Ariadne selects an allowing option automatically. | You accept the agent's requested tool access by default. |
| `ask` | Ariadne shows every permission request in the console and waits for your answer. | You want to approve or deny each request yourself. |
| `learn` | Ariadne asks the first time, then remembers an allowing answer for a matching request. | You want review at first use without repeating the same approval. |
| `ai` | The AI permission model decides first; an uncertain answer falls back to `learn`. | You want local model review with remembered console approvals as a fallback. |

For `ask` and a new `learn` request, `ariadne attention` marks the session as
waiting. Open it with `ariadne attach <session-id>`. The console shows the
choices as a picker: move with ↑ and ↓ or press a number key, and Enter
answers. With stdin or stdout redirected the choices are a numbered list
instead, and the number typed on a line answers it. The desktop app draws the
same console in a terminal pane, so the same picker and the same keys answer
it there. All of them send the selected answer to the same session.

`auto` chooses an allowing option when one exists. If the request offers no
options, it is cancelled. In `learn`, Ariadne remembers only an allowing
answer. A denial is never saved.

## What `learn` remembers

`learn` keeps an approval per repository, tool name, and tool kind. An approval
for one repository does not grant it in another. The memory survives a daemon
restart and is used only for later requests with the same three values. Change
the repository to `ask` when you want to review a matching request again.

## The AI permission model

The AI permission model is the model behind the `ai` mode. It runs on your own machine: nothing
about a permission request leaves it. It is a Python package, and Ariadne
installs it for you.

For each request, the model sees the tool title and kind, up to 2,000 characters
of its JSON input, the repository path, and the available option names. An
`allow` is confident when its confidence meets the configured threshold. Any
other answer, an unavailable model, or a failed request falls back to `learn`:
an existing approval is used, or the console asks you. The model's own allows are
never remembered; only allowing console answers are. The answered console
line names the model and its confidence when it decided.

Turn it on, look at it, and install it again:

```sh
ariadne permissions ai enable
ariadne permissions ai show
ariadne permissions ai refresh
ariadne permissions ai disable
```

In the desktop app, the same settings are on the **Permissions** screen.

Enabling installs three things, under `~/.ariadne/ai-permissions`:

- a Python virtual environment, in `~/.ariadne/ai-permissions/venv`;
- the package and its dependencies;
- the checkpoints the model decides with, in `~/.ariadne/ai-permissions/hf`.

It needs **Python 3.10 or newer**. Ariadne uses `python3` from the daemon's
own `PATH`, or whatever `python_bin` in `~/.ariadne/config.toml` names — see
[Configuration](configuration.md). Nothing is installed into that
interpreter itself: everything goes into the virtual environment. Enabling
the model against an older Python is refused before anything is downloaded, and
the refusal says which version it found.

The install runs in the background and takes minutes. `ariadne permissions ai
enable` answers at once, and `ariadne permissions ai show` says where it has got
to: `installing`, `ready`, or `failed` with the reason. An install that fails
leaves the one before it on disk, so a working model stays working.

Two more settings:

```sh
ariadne permissions ai set --threshold 0.6      # how sure the model has to be, 0 to 1
ariadne permissions ai set --schedule 03:30     # install again daily, local time
ariadne permissions ai set --no-schedule        # and stop doing that
```

The threshold is how sure the model has to be before its answer is taken; it
defaults to `0.8`, and anything outside 0 to 1 is refused. The schedule is
`HH:MM` in 24-hour local time, and the daily refresh downloads the latest
release and the checkpoints again. It runs once per local date: if the daemon
was down at the scheduled time, it catches up on its next start that day. A
refresh already in progress is not queued. The model starts without one, and then
nothing is downloaded until you ask for it.

The AI permission model uses built-in prompts and the English checkpoint. The
configuration is measured by `bench/ai-permissions/`.

Once the install is ready, the daemon runs the model's local server on a loopback
port and keeps its selected weights in memory for permission decisions. It
stops that child when you disable the model or the daemon exits, and starts it
again after a refresh. Turning the model off keeps every file. Turning it back on
is the release check and nothing more, so it is quick.

Set the AI permission model up before you point a repository at it. A repository set to `ai`
while the model is off is refused:

```
the `ai` permission mode needs the AI permission model; turn it on with
`ariadne permissions enable` first
```

`enable` and `refresh` answer at once, with the install running in the
background; add `--wait` to block until it leaves `installing` instead of
polling `permissions ai show` by hand:

```sh
ariadne permissions ai enable --wait
ariadne permissions ai refresh --wait
```

`--wait` exits 1 and prints `last_error` if the install settles on `failed`.
`ariadne doctor` reports the same two things `permissions ai show` does — the
Python interpreter found and where the install stands — next to the rest of
the daemon's environment; neither ever fails the report, since `ai` is one
permission mode among four.

In the desktop app, the **Permissions** screen holds the same settings, in one
card: a switch for `enabled` — disabled, with the Python version it found (or
that it found none), while there is no Python 3.10 or newer to install into —
a number field for the threshold, and a time field for the daily refresh
whose clear button is what turns it off. A Refresh button reruns the install,
disabled while the model is off or already installing. Below them, a fact
list shows the state, the installed and latest release, whether the
checkpoints are on disk, the endpoint, and when the install last ended well;
the last error, once there is one, shows in the same style a failed task
does. Every control sends its change as it is made, and a refusal shows in a
toast. Setting a repository to `ai` before that is refused on the field
itself, pointing back at this screen rather than at the CLI command above.
