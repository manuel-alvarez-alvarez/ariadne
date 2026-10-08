# The forge integration

Ariadne works with the forge a repository's remote is on — GitHub through
`gh`, GitLab through `glab` — once you enable it. Enabled, a request a task
opens is kept by that task's author until a human merges it, and a request
asking for your review gets a session that reviews it; no agent runs a
forge command itself, and none approves or merges anything.

## Enabling it

```sh
ariadne repo update <repo-id> --forge on \
    --review-model claude-acp:<model-id> --review-effort balanced
```

or on registration, with `ariadne repo add <path> --forge on ...`. Enabling
needs the forge's CLI signed in to the remote's host already — `gh auth
login` for GitHub, `glab auth login` for GitLab — and probes it with `auth
status`; a CLI that is not installed or not signed in refuses the enable
with 409 `forge_unauthenticated` and the CLI's own message. With no remote
Ariadne can read a forge off, the enable is refused with 409
`forge_unavailable`.

On success Ariadne stores the login the CLI is signed in as: `gh api user
--jq .login`, or the `username` `glab api user` answers. Every comment and
every review Ariadne posts from here on is posted as that account, never as
you. `ariadne repo inspect <repo-id>` prints the detected remote, whether
the integration is on, and the stored login; `ariadne repo ls` has a FORGE
column, `<kind> <owner>/<name> on|off`. The desktop app's repository dialog
has one switch, "GitHub integration" or "GitLab integration", once a remote
was detected, with the detected repository, the CLI and the login beside
it.

The same checkout — the same host, owner and repository name — can only be
enabled on one registered repository row at a time, even registered twice
under two base branches: see [Troubleshooting](#troubleshooting).

## The review pin

Enabling turns no review on by itself. One pin, a model and an effort,
decides whether review sessions run at all:

| Flags | Starts a session on |
| --- | --- |
| `--review-model`, `--review-effort` | every open request asking for your review |

With no model pinned no review session starts, and a request asking for
your review gets no detail fetch — Ariadne still lists it, but reads none of
its comments or checks. An empty model (`--review-model ""`) clears the pin
and its effort. An effort flag on its own only moves the effort of a model
already pinned — given with no model pinned, it is refused — and
`--review-effort default` clears the effort alone. The pin is set from the
CLI only: the desktop app's repository dialog has just the switch that
enables the integration.

A request of yours needs no pin: the author of the task that opened it
keeps it, on that author's own model.

## The daemon talks to the forge, never the agent

Every call to `gh` or `glab` — opening a request, reading its comments and
checks, posting a reply or a review, registering a webhook — is the
daemon's own, run as a child process on `gh_bin` or `glab_bin` (see
[Configuration](configuration.md)). No agent ever runs the forge's CLI
itself: a landing briefing that opens a request names no forge command, and
an author keeping its request, or a `pr-reviewer` session, reaches the forge
only through the tools its session lists — never a shell.

## How an author keeps its request

A task with the `pull_request` landing, and the final task of a
`feature_branch` goal, does not end when its request opens. Its author keeps
the request until a human merges or closes it, with the `pr-babysit` skill
Ariadne loads for every such author, and the task stays `approved` until
then. Opening it needs the integration on: with it off, `open_pull_request`
is refused with 409 `forge_disabled`, since nothing would read the request
for the author.

The author never polls. The daemon reads the forge and prompts the author's
own session each time the request has news — a new comment, a check that
turned to failure or back to green, a base branch ahead of the head, a
changed review decision, a merge, or a close — and leaves an idle author
alone in between.

The author **replies to every unanswered comment** — saying what it
changed, or why the code stays — and **resolves no thread**: resolving a
thread, once its reply satisfies you, is yours to do. A change a comment or
a failed check asks for goes onto the branch as a new commit and through the
task's own reviewers with `request_review` before it is pushed. A moved base
branch is merged in rather than rebased onto, and the branch is never
amended, rebased or force-pushed. It reports the request ready once every
required approval and check reads green, and takes the readiness back down
the moment a later change turns one back. Once you merge the request, the
author brings the base branch up to date and finishes the task; a request
closed unmerged fails it. Should the author not finish it once it has read
the merge, Ariadne finishes the task itself and stops the author's agent.

A request of yours that no task opened — one you opened by hand — is listed
too, and nobody keeps it.

## What a `pr-reviewer` session does

A request that asks for your review, once it leaves draft, gets a session
of its own, staffed on the review pin, in a worktree detached at the
request's head rather than a branch of its own. The daemon wakes it with the
request, then again with later pushes and replies in the threads it
opened. Those wait until the request has been quiet for five minutes, and
each new push or reply starts the wait again, so a burst of activity
reaches the reviewer as one prompt rather than one each.

You can ask for the same on a request of your own: open it in the desktop
app's Pull requests tab and press **Start review** in its panel. The dialog
asks what the reviewer runs on — a model and an effort, not the
repository's review pin — and which skills it loads: `pr-reviewer`, its
own playbook, is always on and is all it starts with; any other skill can
join it. The
same is `PUT /v1/pull-requests/<id>/ariadne-review` with `{"asked": true,
"model": "<agent:model>", "effort": "<effort>", "skills": [...]}`. That
session hears of pushes alone — the comments are the author's news — and
its review is always posted as a comment, since no forge takes a change
request from a request's own author. Starting it closes the dialog and, once the daemon
has the agent up, opens that session's console in the panel. **Stop
review** takes it down. The
panel's Sessions tab lists the review session, and the task's author where
a task opened the request; picking one opens its console in the panel.

It posts its findings at one of three priorities:

| Priority | Meaning |
| --- | --- |
| P0 | breaks behavior, data or security; must change before the request lands |
| P1 | a defect, or a missing proof that will bite later |
| P2 | worth fixing |

It posts one review per round — `request_changes` where a P0 finding
exists, `comment` otherwise — never an approval. The review's own text is a
short verdict: how many findings of each priority and their titles, tied to
no line and saying nothing of what the reviewer did. Each finding is an inline comment of its own on
the line of the defect, opening on **[P0] Title**, then what goes wrong and
how to fix it. A change request whose P0 findings are not on their lines is
refused. On GitHub that is one API
call carrying every inline comment; on GitLab, which has no review verdict
of its own, one discussion per comment and a summary note that opens with
"Request changes" when it asks for changes. On a request a task's author keeps, the review's
findings go to that author like anyone else's comments, though they are
posted under your login: it answers each one, and fixes what needs fixing.

## Nothing approves or merges in your name

No session can post an approval or merge a request — there is no tool for
either. `pr-reviewer` posts at most a comment; the approving call, and the
merge, are always yours. The one thread a session resolves is a review
thread it opened itself: on a later round, once a push fixed that finding,
it replies and resolves it. Every thread anyone else opened stays yours to
resolve.

## Readiness and review flags on `ariadne attention`

An author that just reported its request ready shows in `ariadne attention`
and the desktop app's attention strip as waiting on you — every required
approval and check reads green, and the merge is yours. A review session that has just posted shows
as "review posted, approve yourself" in the CLI and **Review posted,
approve yourself** in the desktop app — the review needs nothing further,
but your approval does. Either flag opens that session's console, the same
as any other row on `ariadne attention`.

## Webhooks: a delivery wakes the daemon, the daemon wakes the agent

Every daemon runs a webhook listener, bound to `webhook_listen` (default a
random free loopback port) and separate from the REST API: it serves only
`POST /webhooks/github/<repository-id>` and
`POST /webhooks/gitlab/<repository-id>`, and nothing else — no `/v1`, no
documentation. Each delivery is checked against the integration's own
secret, constant-time: GitHub's `X-Hub-Signature-256`, GitLab's
`X-Gitlab-Token`. A missing or wrong one gets 401 and triggers nothing. An
authenticated delivery gets 202 and wakes one fetch of the repository — it
never applies the delivery's payload directly, so GitHub and GitLab wake the
same way, and a delivery Ariadne missed costs nothing more than the next
one.

Ariadne registers the hook itself, once it has a public URL to give the
forge: `webhook_public_url` in `config.toml`, or the tunnel below.
Reconciliation creates a missing hook and updates one whose URL changed,
with no step of yours on the forge.

Each enabled integration's webhook sits in one of three states:

| Webhook state | What it means | Fetch mode |
| --- | --- | --- |
| `live` | the hook is registered and reachable | `WakeOnly`: a delivery wakes the next fetch, no timer |
| `polling` | no live hook yet — nothing gives the forge a URL to register against (no `webhook_public_url`, and the tunnel down or switched off), or the forge refused the last registration | `Timer`: an immediate fetch, then one every 5 minutes |
| `failed` | a hook that was `live` broke on a later update or deletion the forge refused, while its URL still reached it | `Timer`, same as `polling` |

So a delivery that never arrives, a tunnel that is down, or a hook the forge
refuses still leaves the request moving on its timer fallback, just slower.
`repo inspect` prints the hook's `state`, `url`, `error`, the time of the
last delivery and the last fetch failure under the forge block; the
repositories table shows them as one pill, below.

Each fetch reads every open request of the repository, and which of them
ask for your review, and the open issues too: where the issues moved since
the last read, the desktop app reads them again on its own.

## The tunnel

With no `webhook_public_url` configured, Ariadne can open its own tunnel so
a forge on the public internet reaches your webhook listener without one —
the `localtunnel-client` crate, no Node or `lt` binary needed.

```sh
ariadne forge tunnel
ariadne forge tunnel on
ariadne forge tunnel off
```

`ariadne forge tunnel` with no argument prints `switch` (`on` or `off`),
`state` (`up <url>`, `down`, or `off`), `listen` (the bound listener
address), `since` and `error` (each `-` where there is nothing to show). In the
desktop app, **Settings** (the gear in the header) has the **Webhook tunnel**
switch, what it does, and its state: "Tunnel up" with the URL, "Tunnel down"
with the error, or "Tunnel off". The repositories table's Webhook column
shows a pill for each enabled repository: `localtunnel` while the tunnel is
switched on, `polling` while it is off. It is green while that works and red
when it does not; hover it for the hook URL, the error, or how to turn the
tunnel on. `ariadne repo inspect` prints the same, with the last fetch
failure as `fetch error`.

Where nothing gives the forge a fixed URL, the tunnel's own state decides
which webhook state above an enabled integration can reach: `up` lets
reconciliation register the hook and reach `live`; `down` holds every
integration on `polling`, since there is nowhere for the forge to deliver
to. `off` by the switch does the same. `off` because `webhook_public_url` is
set is different: the tunnel itself runs nothing, but reconciliation
registers the hook against that fixed URL instead, and an integration
reaches `live` and `WakeOnly` through it the same as through an open tunnel.
`failed` is a third kind of trouble: a hook that was `live` broke on a
later update or deletion the forge refused, while a public URL still
reached it. It does not survive a tunnel outage — the moment that URL goes
away, reconciliation moves the integration to `polling` instead, with the
outage as its recorded error, overwriting whatever `failed` held; see
[Troubleshooting](#troubleshooting).

Off by the switch closes the tunnel and leaves every hook registered with
the old URL — the switch creates and deletes nothing on the forge by
itself, it only decides whether the URL behind an existing hook is
reachable. A tunnel that drops reconnects on its own, forever, with a
backoff starting at one second and capped at 60; once it is back, Ariadne
updates the hook's URL if it moved and fetches once to catch up on whatever
a delivery could not reach it with.

## The Pull requests tab and adding a request by hand

The desktop app's **Forge** screen opens on its Pull requests tab, which
lists every open request of your enabled repositories. **All**, **Mine**
and **Review requests** narrow it; a repository picker and a text box —
the number, title, description, author or branch — narrow it further, and
every filter stays in the address. A row shows its checks and review
decision as pills, "by you" on a request of yours, and **Ariadne
reviewing** while Ariadne reviews one of yours. A row holds no button: a
click on it opens the request's panel — its state and checks, its facts with
links to the forge and to each failed check, its description rendered, and
its sessions — where **Start review**, **Stop review** and, on a request
added by hand, **Remove** are. The list updates on its own as each fetch
finds a change. While a repository in view polls, or its webhook is down,
**Refresh** asks for a fetch now; with every webhook live there is nothing
to refresh, and the button is gone.

```sh
ariadne pr ls --repo <repo-id>
ariadne pr ls --mine
ariadne pr ls --review-requests
ariadne pr add https://github.com/owner/repo/pull/42
ariadne pr rm <id>
```

`ariadne pr ls` lists every open request, `--mine` your own and
`--review-requests` the ones that ask for your review, as the tab's
three choices do. A request
someone else opened, from a repository with the integration off or one the
lists miss, can be added by hand from the CLI alone; a request of your own
is refused with 409 `pull_request_is_yours`, since the fetch lists it.
`ariadne pr rm` and the panel's Remove button only work on one you added by
hand; a request the forge found cannot be removed this way.

## The issues screen and `goal create --from-issue`

```sh
ariadne issue ls --repo <repo-id>
ariadne issue ls --repo <repo-id> --all
```

lists open issues from an enabled repository — by default only the ones
assigned to the login the integration stored, or every open one with
`--all`. The Issues tab of the desktop app's **Forge** screen does the same,
with a repository filter, an "Assigned to me" switch on by default, a text
box over the title and description. It reads the issues again on its own
when a fetch finds they moved, and offers **Refresh** only while a
repository in view polls or its webhook is down.

```sh
ariadne goal create --from-issue https://github.com/owner/repo/issues/7 \
    --model claude-acp:<model-id>
```

reads the issue and uses its title and body where `--title` and `-d` are
not given, and keeps the issue's URL on the goal: the orchestrator is
briefed to require every author's request body to say `Closes <url>`, so
merging the request closes the issue with it. `ariadne goal inspect` and the
goal panel both link the issue. In the desktop app, **Create goal** on an
Issues row opens the same goal dialog, already filled in.

## Troubleshooting

**Enabling, or a later edit, is refused with `forge_unauthenticated`.**
The forge's CLI is not installed, or not signed in to the remote's host.
Run `gh auth login` or `glab auth login` for that host, then enable again;
the refusal carries the CLI's own message, not a generic one.

**The webhook reads `polling` or `failed`, with an `error` about rights.**
Registering a webhook needs admin rights on the repository — a broader
grant than opening requests or posting reviews needs. Sign in as, or ask,
an account that administers the repository, then let reconciliation retry;
until then the repository keeps working on its `Timer` fallback, nothing
is lost, only slower.

**The tunnel reads `down`.**
This is Ariadne retrying on its own with a capped backoff, not a fixed
failure: every enabled repository keeps fetching on its timer in the
meantime. If it never comes back, check your network can reach
`tunnel_host` (default `https://localtunnel.me`); a firewall or proxy that
blocks outbound connections to it keeps the tunnel down forever.

**The tunnel reads `off`.**
With `webhook_public_url` set in `config.toml`, this is expected and nothing
to fix: the tunnel opens no URL of its own, whatever the switch says, and
hooks register against that fixed URL instead. With no such key, it is
either the switch (`ariadne forge tunnel off`) or no repository having the
integration enabled yet — turn the switch back on, once something needs
it.

**Enabling is refused with 409, naming another repository.**
The same checkout's remote — the same host, owner and name — is already
enabled on that repository row, commonly the same checkout registered a
second time under another base branch. Disable the integration there first,
or work from that row instead of registering a second one: one forge
repository is enabled on one row at a time.
