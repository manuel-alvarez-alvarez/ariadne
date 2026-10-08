# The forge integration

Ariadne works with the forge a repository's remote is on — GitHub through
`gh`, GitLab through `glab` — once you enable it. Enabled, a request of
yours gets a session that answers it until a human merges it, and a request
asking for your review gets a session that reviews it; neither runs a forge
command itself, and neither approves or merges anything.

## Enabling it

```sh
ariadne repo update <repo-id> --forge on \
    --babysit-model claude-acp:<model-id> --babysit-effort balanced \
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
has the same facts in its Forge section, read-only until there is a
checkout to detect them from.

The same checkout — the same host, owner and repository name — can only be
enabled on one registered repository row at a time, even registered twice
under two base branches: see [Troubleshooting](#troubleshooting).

## The two pins

Enabling turns nothing on by itself. Two roles, each its own model and
effort, decide which sessions run at all:

| Role | Flags | Starts a session on |
| --- | --- | --- |
| babysit | `--babysit-model`, `--babysit-effort` | every open request of yours |
| review | `--review-model`, `--review-effort` | every open request asking for your review |

A role with no model pinned starts no session for it, and its requests get
no detail fetch — Ariadne still lists them, but reads none of their
comments or checks. An empty model (`--babysit-model ""`) clears the role
and its effort. An effort flag on its own only moves the effort of a model
already pinned — given with no model pinned, it is refused — and
`--babysit-effort default` or `--review-effort default` clears the effort
alone. In the desktop app, the repository dialog's Forge section has a pin
picker and a Clear button for each, labelled "Babysitter runs on" and
"Reviewer runs on".

## The daemon talks to the forge, never the agent

Every call to `gh` or `glab` — opening a request, reading its comments and
checks, posting a reply or a review, registering a webhook — is the
daemon's own, run as a child process on `gh_bin` or `glab_bin` (see
[Configuration](configuration.md)). No agent ever runs the forge's CLI
itself: a landing briefing that opens a request names no forge command, and
a `pr-babysit` or `pr-reviewer` session reaches the forge only through the
tools its session lists — never a shell.

## What a `pr-babysit` session does

A task with the `pull_request` landing ends the moment its request is open
and its branch is pushed: the request's comments, checks and merge are not
its to wait on. From there, a request of yours — one a task opened, one the
forge found, or one you added by hand — gets a session of its own, staffed
on the babysit pin above, with no goal and no task behind it. It has no
timer either: the daemon wakes it with a prompt each time the request has
news — a new comment, a check that turned to failure or back to green, a
base branch ahead of the head, a changed review decision, a merge, or a
close.

That session **replies to every unanswered comment** — applying the fix it
asks for, or saying why the code stays — and **resolves no thread**:
resolving a thread, once its reply satisfies you, is yours to do. It fixes a
failed check and pushes the fix forward, merges a moved base branch in
rather than rebasing onto it, and never amends, rebases, or force-pushes the
branch the request was opened from. It reports the request ready once every
required approval and check reads green, and takes the readiness back down
the moment a later change turns one back.

## What a `pr-reviewer` session does

A request that asks for your review, once it leaves draft, gets a session
of its own too, staffed on the review pin, in a worktree detached at the
request's head rather than a branch of its own. The daemon wakes it with the
request, then again with every later push and with a reply in a thread it
opened.

It posts its findings at one of three priorities:

| Priority | Meaning |
| --- | --- |
| P0 | breaks behavior, data or security; must change before the request lands |
| P1 | a defect, or a missing proof that will bite later |
| P2 | worth fixing |

It posts one review per round — `request_changes` where a P0 finding
exists, `comment` otherwise — never an approval. On GitHub that is one API
call carrying every inline comment; on GitLab, which has no review verdict
of its own, one discussion per comment and a summary note that opens with
"Request changes" when it asks for changes.

## Nothing approves or merges in your name

Neither session can post an approval, merge a request, or resolve a thread —
there is no tool for any of the three. `pr-reviewer` posts at most a
comment; the approving call, and the merge, are always yours.

## Readiness and review flags on `ariadne attention`

A babysit session that just reported ready shows in `ariadne attention` as
"ready to merge", and in the desktop app's attention strip and session
views as **Ready to merge** — every required approval and check reads
green, and the merge is yours. A review session that has just posted shows
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
| `polling` | no live hook yet — nothing gives the forge a URL to register against (no `webhook_public_url`, and the tunnel down or switched off), or the forge refused the last registration | `Timer`: an immediate fetch, then one every 60 seconds |
| `failed` | a hook that was `live` broke on a later update or deletion the forge refused, while its URL still reached it | `Timer`, same as `polling` |

So a delivery that never arrives, a tunnel that is down, or a hook the forge
refuses still leaves the request moving on its timer fallback, just slower.
The repositories table and form show the same four facts read-only —
`state`, `url`, `error` and the time of the last delivery — and `repo
inspect` prints them under the forge block.

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
address), `since` and `error` (each `-` where there is nothing to show). The
repositories screen shows the same state next to a **Webhook tunnel** switch
in its header, labelled "Tunnel up", "Tunnel down" or "Tunnel off"; hover it
for the URL, what it forwards to, and since when.

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

## Adding a request by hand

A request the forge opened before Ariadne tracked it, or one from a
repository with the integration off, can still be followed:

```sh
ariadne pr add https://github.com/owner/repo/pull/42
ariadne pr ls --repo <repo-id>
ariadne pr rm <id>
```

or from the desktop app's Pull requests screen, **Add pull request**, which
searches an enabled repository's open requests live and also accepts a URL.
A hand-added request gets a babysit or review session the same as one a
task opened or the forge found, on whichever pins its repository has set.
`ariadne pr rm` and the panel's Remove button only work on one you added by
hand; a request a task opened or the forge found cannot be removed this
way.

## The issues screen and `goal create --from-issue`

```sh
ariadne issue ls --repo <repo-id>
ariadne issue ls --repo <repo-id> --all
```

lists open issues from an enabled repository — by default only the ones
assigned to the login the integration stored, or every open one with
`--all`. The desktop app's Issues screen (`#/issues`) does the same, with a
repository filter and an "Assigned to me" switch on by default.

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
