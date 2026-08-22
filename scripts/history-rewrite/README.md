# History rewrite

Tooling that turns `main`'s history — 526 commits, 178 of them merges, and area
prefixes (`ui:`, `daemon:`, `cli:` …) where release-please expects conventional
commit types — into a linear history whose every subject release-please can
read. See [`.github/RELEASING.md`](../../.github/RELEASING.md) for what it does
with them.

Nothing here pushes, and nothing here moves `main`: `linearize` writes commit
objects and, at most, one local branch you name. The cutover — force-pushing
the rewritten history over `main` — is a separate task.

## The three pieces

| File              | What it is                                                            |
| ----------------- | --------------------------------------------------------------------- |
| `linearize`       | replays a ref's non-merge commits onto one another, rewriting subjects |
| `message-map.tsv` | `<full sha><TAB><new subject>`, one line per commit that needs one     |
| `verify`          | checks a rewritten ref against the original, five ways                 |

## Usage

```sh
# Rewrite main and keep the result in a local branch.
scripts/history-rewrite/linearize main --branch rewritten

# Check it against what it was rewritten from.
scripts/history-rewrite/verify main rewritten
```

`linearize` prints the new tip sha on stdout and its progress on stderr, so
`tip=$(scripts/history-rewrite/linearize main --quiet)` is the scripted form.
Without `--branch` nothing but loose objects is created; the ref you name is
the only thing written, and an existing branch of that name is overwritten.

`verify` exits non-zero, naming what broke, unless all five hold:

1. the new history has no merge commits;
2. it has as many commits as the old one has non-merge commits;
3. its tip tree is byte-identical to the old tip tree;
4. every subject matches the conventional-commit grammar;
5. pairwise, in `--topo-order`, each new commit carries the author name, email
   and author date of the original commit it stands for.

## How the rewrite works

`git rev-list --reverse --topo-order --no-merges <ref>` is the input: merge
commits are dropped, everything else is replayed in order with
`git commit-tree`, each replacement taking the previous one as its only parent.

Every replacement reuses the **original commit's tree**. Nothing is applied or
merged, so the run cannot conflict and the final tree is guaranteed identical
to `<ref>`'s — check 3 above. The accepted trade-off: while a commit that was
written on a side branch keeps its own tree, the diff *against its new parent*
can transiently show changes that came from a sibling branch. Per-commit
authorship, dates and content are exact; only the intermediate diffs shift.

Author name, email and date are preserved, and the committer date and identity
are taken from the original commit, so a run is deterministic — same ref and
same map give the same tip sha, on any machine.

## The pass-through rule

A commit's new subject comes from `message-map.tsv` when its sha is listed
there. When it is not, the original subject is passed through **only if it
already matches**

```
^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9,./ -]+\))?!?: .+
```

and otherwise `linearize` fails, naming the sha and the subject it will not
guess at. The message body and its trailers are always kept verbatim below the
subject.

So commits that land on `main` after this map was written need no maintenance
as long as they are conventional — which is what the repository asks for now
anyway. A commit that is not has to be added to the map before the rewrite can
run. The same rule makes a map row a deliberate act: a typo'd sha is a row that
matches nothing, and its commit is then judged by its own subject.

`message-map.tsv` is read and checked in full before the first commit is
written: a line without a tab, a sha that is not 40 hex characters, and a new
subject that is not itself conventional are each a hard error. Blank lines and
`#` comments are ignored.

## Writing a map row

The subject becomes a line in the release notes, so it is written for whoever
reads the changelog, not for whoever wrote the patch:

- `type(scope): imperative summary`, classified by what the commit actually
  changes — `git show --stat`, and the diff when the old subject is vague.
- `feat` for a new user-visible capability, `fix` for a bug fix; `perf` and
  `revert` as they say. `docs`, `refactor`, `test`, `chore`, `style`, `build`
  and `ci` are hidden from the notes and are where everything internal goes.
- The old area prefix is usually the scope: `ui`, `daemon`, `cli`, `store`,
  `core`, `api`, `scripts`, `prompts`. A change spanning several layers takes
  no scope rather than an invented one.
- `!` and `BREAKING CHANGE:` are for commits that genuinely broke
  compatibility, which pre-1.0 essentially none of these did.
