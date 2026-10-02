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
| `learn` | Ariadne asks the first time, then allows a matching request that you allowed before. | You want review at first use without repeating the same approval. |
| `ai` | The AI permission model decides first; an uncertain answer falls back to `learn`. | You want local model review with remembered console approvals as a fallback. |

For `ask` and a new `learn` request, `ariadne attention` marks the session as
waiting. Open it with `ariadne attach <session-id>`. The console shows the
choices as a picker: move with ↑ and ↓ or press a number key, and Enter
answers. With stdin or stdout redirected the choices are a numbered list
instead, and the number typed on a line answers it. The desktop app draws the
same console in a terminal pane, so the same picker and the same keys answer
it there. All of them send the selected answer to the same session.

`auto` chooses an allowing option when one exists. If the request offers no
options, it is cancelled. In `learn`, only an allowing answer allows a later
request. A denied request is asked again.

## What `learn` remembers

Ariadne records every answer you give in the console, in every mode, allow or
deny. It also records every denial of the AI permission model and of its
rules. These rows are training data for the permission model. An allow that
Ariadne made without you (by the model, by a learned row, or in `auto`) is not
recorded.

A row is kept per repository, tool name, and tool input. The tool name is the
agent's own name for the tool, such as `Bash`, `Read` or
`mcp__ariadne__create_task`, or else the title of the call. The tool input is
the `rawInput` of the call. So an allowed `Bash` command allows only that same
command, not another one. A later answer for the same request replaces the
answer in its row.

In `learn` and `ai` only, a row whose answer allowed the request allows a later
request with the same repository, tool name, and input, without asking you. A
row whose answer denied the request never answers it: the request is asked
again. A request without `rawInput` is recorded but never allowed from a row.
An approval for one repository does not grant it in another. The rows survive
a daemon restart. Change the repository to `ask` when you want to review a
matching request again.

Each row keeps the complete ACP tool call, the offered options, the selected
option, the permission mode at the time (`target`), and the model decision
(`output`) when the AI permission model was called.

Manage these rows with `ariadne permissions learned list`, `show`, and `rm`.
You cannot add or edit a row by hand.

## The AI permission model

The AI permission model is the model behind the `ai` mode. It runs on your own machine: nothing
about a permission request leaves it. It is a Python package, and Ariadne
installs it for you.

For each request, Ariadne derives an operation and ordered risk tags from the
complete tool call. The model sees the workspace, the tool call's title and
kind, the compact JSON of its raw input (cut at 2,000 characters), the option
names, and the risk tags. Empty values are left out. The operation is reported
with the decision but is not sent to the model.

The model answers one question: “How much does this coding-agent tool call put
the system or the project at risk?” It scores the request as routine work to
allow, consequential work to ask about, or unacceptable work to deny. Routine
work includes reading, listing, and searching files outside the workspace when
the request has no credential and transfers nothing.

The answer is a danger score: the expected level, from 0 for safe through 1
for dangerous. Ariadne
allows a request at or below the allow threshold, denies one at or above the
deny threshold, and asks between them. A denial selects only a one-time reject
option. When the agent offers no one-time reject, the console asks instead;
the model never selects a permanent rejection. A model allow becomes an ask when the request has the
`reviewer_directive` tag. This is the only cap. No tag denies a request.

An ask, an allow without an allowing option, an unavailable model, or a failed
request falls back to `learn`. An allowed row answers; otherwise, the console
asks you. Every console answer and every model denial is recorded. A model
allow is not recorded.

The model returns allow, ask, and deny probabilities. The console shows the
allow probability from level `0` and the deny probability from level `2`.
It shows the decision, both probabilities, danger, and both thresholds, each
as a whole percent. It shows the reason under a waiting call. `ariadne session
logs` prints the same reason under the question. The answer and event summary
use these words:

```text
AI allowed · allow 99%, deny 0% · danger 1% (allow up to 5%, deny from 65%)
AI denied · allow 1%, deny 88% · danger 93% (allow up to 5%, deny from 65%)
allow-once in the console — AI said ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%)
allow-once in the console — AI said ask · allow 96%, deny 1% · danger 2% (allow up to 5%, deny from 65%) · tags: reviewer_directive · capped by reviewer_directive
allow-once in the console — AI unavailable
```

`AI said allow` means no allowing option was available. `AI said deny` means
no one-time reject option was available. `AI said ask` means the danger fell
between the thresholds or the cap held an allow. `AI unavailable`, `failed`,
`timed out`, and `malformed` mean the model gave no answer. The daemon log
records each decision with its fields.

Turn it on, look at it, and install it again:

```sh
ariadne permissions ai enable
ariadne permissions ai show
ariadne permissions ai refresh
ariadne permissions ai disable
```

Test one request before an agent makes it with the same model and thresholds:

```sh
ariadne permissions ai test --tool Bash --kind execute \
  --input '{"command":"git status"}' --option Allow --option Reject \
  --workspace "$PWD"
```

The command prints the label, both probabilities, danger and thresholds as
whole percentages, then the operation. For example:
`ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%); operation read_workspace`.
For a capped allow, the percentages are followed by `· tags: reviewer_directive · capped by reviewer_directive`.
Use `--format json` for the response fields and all model probabilities.
It does not select an option, save an approval, or add an event. The model
must be enabled; `ai_disabled` means turn it on first. Invalid `--input` JSON
is refused locally. If the enabled model cannot answer, the command prints
`no answer: unavailable`, `failed`, `timed out`, or `malformed`.

In the desktop app, the same settings are on the **Permissions** screen.

Enabling installs three things, under `~/.ariadne/ai-permissions`:

- a Python virtual environment, in `~/.ariadne/ai-permissions/venv`;
- the pinned model package and its dependencies;
- the adapter and base model of the chosen flavour, in
  `~/.ariadne/ai-permissions/hf`.

On Linux with the `cpu` device, PyTorch comes from PyTorch's CPU-only index,
so the install does not download the CUDA build. With `cuda`, it comes from
PyPI. A change between `cpu` and `cuda` on Linux reinstalls PyTorch.

It needs **Python 3.12 or 3.13**. Ariadne tries `python3.13`, then `python3.12`,
then `python3` from the daemon's own `PATH`, or whatever `python_bin` in
`~/.ariadne/config.toml` names — see
[Configuration](configuration.md). Nothing is installed into that
interpreter itself: everything goes into the virtual environment. Enabling
the model against an older Python is refused before anything is downloaded, and
the refusal says which version it found.

The install runs in the background and takes minutes. `ariadne permissions ai
enable` answers at once, and `ariadne permissions ai show` says where it has got
to: `installing`, `ready`, or `failed` with the reason. An install that fails
leaves the one before it on disk, so a working model stays working.

### Choosing a flavour and device

Kev publishes four flavours, 0.8B, 4B, 9B and 27B, each of which can run on
Apple Silicon (`mlx`), an NVIDIA GPU (`cuda`), or the CPU. `ariadne permissions
ai show` reports the hardware Ariadne found — the OS, the architecture, the
RAM, and the largest GPU's name and VRAM where there is one — and a table of
every flavour and device with whether it can run there and, when it cannot,
why: `needs 24 GB VRAM, found 8 GB`. A combination the table marks slow still
runs; it is only slower than the same flavour on a faster device.

```sh
ariadne permissions ai set --flavour 9b --device cuda
ariadne permissions ai set --flavour 9b     # keeps mlx, cuda or cpu — the best device that runs it
ariadne permissions ai set --device cpu     # keeps the flavour already chosen
```

A combination this machine cannot run is refused with the same reason the
table shows, and nothing is stored. The default is `4b` on the best available
device, or `0.8b` where nothing on the machine can run `4b`.

A change while the model is on reinstalls at once: `show` says `installing`,
the server stops, and it starts again on the new flavour and device when the
install is ready. A change while the model is off is only stored, and
`enable` installs it. `refresh` repairs the stored choice. After an install
that succeeds, the weights of every other flavour are deleted from
`~/.ariadne/ai-permissions/hf`. An install that fails keeps them.

The server runs on the chosen device. `mlx` uses Kev's MLX backend, `cuda` its
PyTorch backend, and `cpu` its PyTorch backend in 32-bit precision. On Linux,
`cpu` also hides every GPU. On a Mac, `cpu` hides the Apple GPU from PyTorch.
When the server is up, Ariadne checks the device and the backend that it
reports. A mismatch stops the server, and `show` says `failed` with both
values.

The CPU is slow. On an Apple Silicon Mac, one `0.8b` decision on `cpu` took
about 10 seconds, and a decision that takes more than 5 seconds gets no
answer from the model.

Two more settings:

```sh
ariadne permissions ai set --allow-threshold 0.2 --deny-threshold 0.8
```

The allow threshold defaults to `0.0531`, and the deny threshold defaults to
`0.6522`. Both take values from 0 to 1, and the allow threshold must stay below
the deny threshold. Upgrading to the 2026-10-01 winner resets both stored thresholds to this pair.
Lower the allow threshold to ask about more requests.
Lower the deny threshold to reject more dangerous requests without asking.
Set either or both with `ariadne permissions ai set --allow-threshold <value>
--deny-threshold <value>`. Refresh is manual only: `ariadne permissions ai
refresh` reinstalls the same pinned package, adapter and base to repair them,
and nothing runs it on a schedule.

The [AI permission benchmark](../bench/ai-permissions/README.md) selected
Kev-4B with the three-level score question, temperature 0.6, derived risk
tags, one `reviewer_directive` cap, and thresholds 0.0531 and 0.6522.
On its audited sets, the pair allowed 381 of 645 safe cases and 141 of 299
real requests. It allowed no elevated or adversarial case. It denied no safe
or real request.

The real-request sample changes over time.
The model reads one request at a time and sees at most 2,000 characters of its
input, so a later command can remain invisible. Scores can vary near a bound
with another device or numeric precision.

Once the install is ready, the daemon runs the model's local server on a loopback
port and keeps its weights in memory for permission decisions. It
stops that child when you disable the model or the daemon exits, and starts it
again after a refresh. Turning the model off keeps every file. Turning it back on
reinstalls the same pins and repairs the model.

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
that it found none), while there is no Python 3.12 or 3.13 to install into —
and number fields for the allow and deny thresholds. A Refresh button reruns the install,
disabled while the model is off or already installing. Below them, a fact
list shows the state, the installed and latest release, whether the
checkpoints are on disk, the endpoint, and when the install last ended well;
the last error, once there is one, shows in the same style a failed task
does. Every control sends its change as it is made, and a refusal shows in a
toast. Setting a repository to `ai` before that is refused on the field
itself, pointing back at this screen rather than at the CLI command above.
