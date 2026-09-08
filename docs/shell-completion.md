# Shell completion

Completions are **dynamic**: what the shell sources is a few lines that call
`ariadne` back on every TAB, so the candidates are the ones the daemon has
right now — task, goal and session ids with their status and title beside
them, skill names with the line each one says about itself, and the models an
agent can be pinned to. They are verb-aware, too: `task retry` offers the failed tasks,
`session kill` the live sessions, `session resume` the ended ones, `goal rm`
the goals it will actually delete.

[`scripts/install.sh`](../scripts/install.sh) wires this up for bash and zsh —
see [Installing Ariadne](install.md). To do it yourself, or for a shell it
skipped:

```sh
ariadne completions install                 # $SHELL, or --shell bash|zsh|fish
```

or write the line by hand — it is the same one:

```sh
echo 'source <(COMPLETE=bash ariadne)' >> ~/.bashrc
echo 'source <(COMPLETE=zsh ariadne)' >> ~/.zshrc
ariadne completions fish > ~/.config/fish/completions/ariadne.fish
```

`ariadne completions <shell>` prints that registration, so `source <(ariadne
completions zsh)` works in a shell you have open now. A daemon that is down or
slow leaves TAB with nothing rather than an error, and `--model` and
`--effort` complete from a catalog cached under the ariadne home — `--model`
candidates carry the tier, cost and speed beside the description, and
`--effort` candidates carry what that effort buys. For somewhere a completion has
to be a file on disk, `ariadne completions <shell> --static` prints the old
snapshot script, which has the command tree but none of the live candidates.
