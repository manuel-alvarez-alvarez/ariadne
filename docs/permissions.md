# Permission modes

Choose how Ariadne handles an ACP agent's tool-permission questions. The
default is `auto`, which keeps routine work moving. Choose `ask` when you want
to decide each request, or `learn` when repeated approved requests in one
repository should stop interrupting you.

## Set the default

Set `permission_mode` in `~/.ariadne/config.toml`, then restart the daemon.
This applies to every task that does not select its own mode.

```toml
permission_mode = "ask"
```

```sh
ariadne daemon restart
```

Leave the key out to use `auto`.

## Set a task's mode

Pass `--permission-mode` when creating a task. It overrides the daemon default
for that task.

```sh
ariadne task create <goal-id> --title "Update dependencies" \
    --author coding=codex-acp:<model-id> --no-reviewer \
    --permission-mode learn
```

The accepted values are `auto`, `ask`, and `learn`. A task with no
`--permission-mode` uses the configured default.

## What each mode does

| Mode | Behaviour | Use it when |
| --- | --- | --- |
| `auto` | Ariadne selects an allowing option automatically. | You accept the agent's requested tool access by default. |
| `ask` | Ariadne shows every permission request in the console and waits for your answer. | You want to approve or deny each request yourself. |
| `learn` | Ariadne asks the first time, then remembers an allowing answer for a matching request. | You want review at first use without repeating the same approval. |

For `ask` and a new `learn` request, `ariadne attention` marks the session as
waiting. Open it with `ariadne attach <session-id>`. The console displays the
choices; enter its number, id, or name. The desktop console displays the same
choices and sends the selected answer to the same session.

`auto` chooses an allowing option when one exists. If the request offers no
options, it is cancelled. In `learn`, Ariadne remembers only an allowing
answer. A denial is never saved.

## What `learn` remembers

`learn` keeps an approval per repository, tool name, and tool kind. An approval
for one repository does not grant it in another. The memory survives a daemon
restart and is used only for later requests with the same three values. Change
the task to `ask` when you want to review a matching request again.
