# Following what happens

Nothing here polls: the daemon streams, and every follow mode below reads that
stream. Ctrl-C ends any of them and leaves the terminal as it found it.

```sh
ariadne events                         # what the daemon has done, one line each
ariadne events -f                      # ...and keep printing as it happens
ariadne events -f --goal <goal-id>     # one goal's; also --task, --session, --kind
ariadne events -f --format json        # one JSON object per line, for a pipe

ariadne session logs <session-id> -f   # an agent's event transcript, until it ends
ariadne task logs <task-id> -f         # the same, found by task (--seat reviewer)
ariadne daemon logs -f                 # the daemon's own log, over the API

ariadne attention --watch              # redrawn whenever something needs you
ariadne task ls --watch --goal <id>    # redrawn whenever a task moves
ariadne goal ls --watch                # redrawn whenever a goal moves
ariadne session ls --watch --seat reviewer  # redrawn whenever a session moves
```

`-f` prints as it goes; `--watch` redraws the whole table, `watch(1)`-style,
when an event says it has changed. `ariadne daemon logs` reads the daemon's own
ring buffer, so it works under launchd and systemd — where nothing writes
`~/.ariadne/ariadned.log` — and honours `--endpoint`; the file is the fallback
for a daemon that is not answering.
