# Using the CLI

`ariadne` is the whole terminal surface: it creates goals, edits the plan,
attaches to an agent and prints what the daemon knows. This page is the tour;
`ariadne --help` is the reference.

## A first run

```sh
cargo build --release          # builds `ariadned` and `ariadne`

ariadne daemon start           # unix socket at ~/.ariadne/ariadne.sock

# A model is required for every run. Register a repository once and reference
# it from every goal that works in it (--branch defaults to the checked-out
# branch), so this already works:
ariadne repo add ~/projects/api --description "the public API"
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model claude_code:claude-sonnet-5
ariadne goal attach <goal-id>
```

## Skills, repositories, models and endings

```sh
# what an agent can do is the skills it loads; Ariadne ships a catalog and you
# add your own. A shipped skill is reset, one of yours is deleted.
ariadne skill ls
ariadne skill get coding > coding.md   # pipe it out, edit, pipe it back
ariadne skill set coding --file coding.md
ariadne skill reset coding

# a repository is a checkout and a base branch; how work ends in it is the
# task's own, agreed with you when the task is written
ariadne repo add ~/projects/ui --branch next
ariadne repo ls

# an explicit agent CLI, its model, and how deeply it reasons there
ariadne goal create --title "Add rate limiting" --repo ~/projects/api \
    --model codex:gpt-5.6-sol --effort xhigh
ariadne task update <task-id> --model claude_code:claude-opus-5 --effort xhigh \
    --reviewer code-review=codex:gpt-5.6-luna@high
ariadne task update <task-id> --effort default  # at whatever the CLI reasons it at

# how a task ends, and whether it is reviewed at all
ariadne task create <goal-id> --title "Cut 0.6.0" \
    --author release=claude_code:claude-sonnet-5 \
    --no-reviewer --landing none
ariadne task update <task-id> --landing pull-request
ariadne goal complete <goal-id>        # once its tasks are all done
```

## Watching the work

```sh
# watch it run
ariadne attention                      # what is waiting for you, across every goal
ariadne task ls --goal <goal-id>       # what is going on; -a adds the finished work
ariadne task attach <task-id>          # author terminal (or --seat reviewer)
ariadne attach <id>                    # session, task or goal id

# without attaching to a terminal
ariadne models ls --agent codex        # what --model pins, its tier/cost/speed, and the efforts each takes
ariadne models show codex:gpt-5.6-luna # one model's card: what it's for, and what each effort buys
ariadne models disable codex:gpt-5.6-luna  # take it out of use: not offered, and refused as a pin
ariadne models enable codex:gpt-5.6-luna   # and back in
ariadne session ls --attention         # the agents waiting on a human
ariadne session send <session-id> y    # type into a live agent, as the UI does

# lists are for reading and for piping
ariadne task ls -o wide                # every column, however narrow the terminal
ariadne task ls --columns id,title,age # or exactly the ones you name
ariadne task ls -q | xargs -n1 ariadne task inspect
ariadne task history <task-id>         # every status transition, timestamped and coloured
ariadne task messages <task-id> --full # each message whole, through $PAGER
ariadne task diff <task-id>            # coloured, and through $PAGER on a terminal
```

## The reference, and what to run when it breaks

`ariadne --help` lists every command and `ariadne <command> --help` every flag:
that is the CLI reference, and it is the one this binary actually implements.
`ariadne doctor` is what to run when something is not working — it reports what
your shell sees *and* what the daemon sees, because a daemon started by launchd
or systemd carries the PATH its service file was written with. Every command
that prints data takes `--format json`. The no-JSON exceptions hand the terminal
to another program: `attach`, `daemon logs`, `completions` and `setup`.
The daemon serves its full API as OpenAPI at `/api-docs/openapi.json`, Swagger
UI at `/docs`, and a live event stream at `/v1/events/stream`.

## How the output is laid out

Tables are laid out for the terminal they are printed in: the least important
columns are dropped until the row fits, and `-o wide` puts them all back.
`--columns a,b,c` prints exactly the ones you name, `--no-trunc` prints the
cells whole, and `-q` prints one id per line and nothing else — the flag to
pipe a listing into whatever acts on it. `ariadne goal ls`, `task ls` and
`session ls` show what is going on rather than everything there has ever been;
`-a/--all` includes the finished work, and `--status` names the statuses
precisely. A pipe or a file gets every column, since there is no screen to fit.

Statuses are coloured and carry a glyph — `●` running, `○` pending, `✓` done
or ok, `✗` failed or cancelled, `?` waiting on you, `!` a warning worth a look
— so a table reads the same without colour. `--color auto|always|never`
decides, `NO_COLOR` is honoured, and a pipe is plain unless you ask otherwise
(`--color always`). `task diff`, the `logs` snapshots and `task messages
--full` go through `$PAGER` (`less -R`) on a terminal; `--no-pager` streams
them instead.
