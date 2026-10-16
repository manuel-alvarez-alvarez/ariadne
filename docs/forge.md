# The forge integration

Ariadne works with a repository's forge — GitHub, through `gh` — once you
enable it. Enabled, a request a
task's `pr` column opens is kept by that column's agent until a human
merges it, a request asking for your review is yours to start Ariadne's
review of, by hand, whenever you choose, and a request of your own that no
task opened is yours to ask Ariadne to keep by hand too; no agent runs a
forge command itself, and none approves or merges anything.

## Enabling it

```sh
ariadne repo update <repo-id> --forge on
```

or on registration, with `ariadne repo add <path> --forge on`. Enabling
needs `gh` signed in to the remote's host already — `gh auth
login` — and probes it with `auth status`; `gh` not installed, or not
signed in, refuses the enable with 409 `forge_unauthenticated` and its own
message. A remote on `github.com` is GitHub; another host is GitHub
Enterprise once `gh` is signed in to it. With no remote Ariadne can read
as GitHub, the enable is refused with 409 `forge_unavailable`.

On success Ariadne stores the login `gh` is signed in as: `gh api user
--jq .login`. Every comment and every review Ariadne posts from here on is
posted as that account, never as you. `ariadne repo inspect <repo-id>`
prints the detected remote, whether the integration is on, and the stored
login; `ariadne repo ls` has a FORGE column, `<kind> <owner>/<name> on|off`.
The desktop app's repository dialog has one switch, "GitHub integration",
once a remote was detected — while editing, off the repository's own stored
row, and while registering, off a probe of the path field itself, since
nothing is stored yet — with the detected repository, the CLI and the
login beside it. "Supervise posts" sits right below that switch and only
while it is on, in both dialogs.

The same checkout — the same host, owner and repository name — can only be
enabled on one registered repository row at a time, even registered twice
under two base branches: see [Troubleshooting](#troubleshooting).

## Every review starts manually

Enabling turns no review on by itself, and there is no repository-wide
setting that does: every review, whether a request of yours or one asking
for your review, starts the moment you ask for it, on a model and an
effort you pick for that request alone — see [What a `pr-reviewer` session
does](#what-a-pr-reviewer-session-does) below. With nothing asked, a
request asking for your review gets no detail fetch — Ariadne still lists
it, but reads none of its comments or checks.

A request of yours opened by a task needs no asking at all: the agent of
the task that opened it keeps it, on that agent's own model.

## The daemon talks to the forge, never the agent

Every call to `gh` — opening a request, reading its comments and
checks, posting a reply or a review, registering a webhook — is the
daemon's own, run as a child process on `gh_bin` (see
[Configuration](configuration.md)). No agent ever runs `gh`
itself: the briefing that opens a request names no forge command, and the
agent keeping its request, or a `pr-reviewer` session, reaches the forge
only through the tools its session lists — never a shell.

## How the `pr` column keeps its request

A task whose workflow gates its last column on `request-merged` —
`develop-review-pr`'s `pr` column — does not finish when its request opens.
That column's agent keeps the request until a human merges or closes it,
with the `pr-babysit` skill the column stages. Opening it needs the
integration on: with it off, `open_pull_request` is refused with 409
`forge_disabled`, since nothing would read the request for the agent.

The agent never polls: that rule sits in the session rules of its seat, not
in the skill. The daemon reads the forge and prompts its own session each
time the request has news — a new comment, a check that turned to failure
or back to green, a base branch ahead of the head, a changed review
decision, a merge, or a close — and leaves an idle agent alone in between.
A session that starts with the request already open — after a restart —
skips straight to reading that news instead of opening the request again.

The agent **answers each inline review comment once, in the shape it
calls for**: a requested change goes onto the branch as a new commit, with
the changed tests and lint run again before it is pushed, and the reply
names the commit or says why the code stays; a question gets one short
answer; a thanks gets no reply; a follow-up in a thread it already
answered gets a new answer; and a reviewer reply that keeps a finding
open gets no second try — you decide what happens next. It also answers,
once, a comment of yours on the conversation: it cannot reply twice to
the same one, and it cannot reply to its own earlier words there. A
finding that carries a suggestion block the agent accepts is applied as
given, and rewritten only where it fails to compile or pass; one it does
not accept keeps the code, with a reply that says why. It **resolves no
thread**: resolving a thread, once its reply satisfies you, is yours to
do. Before it runs the affected checks, it integrates a moved base branch
as the repository's history rules require. Where those rules require a
linear history, it rebases and pushes with `--force-with-lease`; otherwise
it merges where those rules allow it. It keeps every commit others added,
never uses an unguarded force push, and never amends a pushed commit outside
that rebase. It reports the request ready once every required approval and
check reads green, and takes the readiness back down the moment a later
change turns one back. Once you merge the request, the agent brings the base
branch up to date and completes the column, which finishes the task when
nothing follows it, or hands on to the next column; a request closed unmerged
fails the task. Should the agent fall quiet once it has read the merge, the
daemon advances or finishes the task itself and stops the agent.

A request of yours that no task opened — one you opened by hand — is listed
too, and nobody keeps it until you ask Ariadne to babysit it; see [What a
`pr-babysit` keeper session does](#what-a-pr-babysit-keeper-session-does)
below.

## What a `pr-reviewer` session does

A request that asks for your review, once it leaves draft, gets a session
of its own once you ask for one, staffed on the pin you gave that ask, in a
worktree detached at the request's head rather than a branch of its own.
The daemon wakes it with the request, then at the next reconcile after a
later push or a reply in a thread it opened — told at once, with no wait
for activity to settle. A push also waits for the session to go idle before
the worktree moves to the new head; a reply alone moves no worktree, so it
needs no idle session.

Nothing starts that session on its own: the request is tracked the moment
it is seen, and Needs attention offers **Review needed**, naming the
repository and the request, until you ask. A head branch git itself
refuses as a ref name starts no worktree and no session either: Ariadne
gives up on it at once, the same way it gives up on a session that never
comes back, and Needs attention names the request until you look. You ask the same way on a
request of your own, too: open either kind of request in the desktop app's
Pull requests tab and press **Start review** in its panel. The dialog asks
what the reviewer runs on — a model and an effort — and which skills it
loads: `pr-reviewer`, its own playbook, is always on and is all it starts
with; any other skill can join it. The same is `PUT
/v1/repositories/<repo-id>/pull-requests/<number>/ariadne-review` with
`{"asked": true, "model": "<agent:model>", "effort": "<effort>", "skills":
[...]}`, from the CLI as `ariadne pr review <repo-id> <number> --model
<agent:model>`: `--model` is required, since there is no repository
setting to fall back to. On a request of yours that session hears of
pushes alone — the comments are the keeping agent's news; on an incoming
request it hears of pushes and replies — and its review is always posted
as a comment where the request is your own, since no forge takes a change
request from a request's own author. Starting it closes the dialog and,
once the daemon has the agent up, opens that session's console in the
panel. **Stop review** takes it down. The panel's Sessions tab lists the
review session, and the task's keeping agent where a task opened the
request; picking one opens its console in the panel.

It posts its findings at one of three priorities:

| Priority | Meaning |
| --- | --- |
| P0 | breaks behavior, data or security; must change before the request lands |
| P1 | a proven defect, unmet requirement, or written repository-rule violation |
| P2 | worth fixing |

Each round posts its new findings as one review — `request_changes` while
a P0 or P1 finding is open, `comment` otherwise — never an approval. Each finding
is an inline comment of its own on the line of the defect, opening on
**[P0] Title**, then what goes wrong and how to fix it. Where the fix
replaces the commented line alone, the comment carries a `suggestion`
block, holding the new text of that one line, the author can apply
directly; a fix across lines, across files, or one that needs a test,
stays prose. A change request whose P0 or P1 findings are not on their lines is
refused. This is one API call carrying every inline comment.

The review posts no generated summary, status table, summary comment, or
summary draft. A round with no new finding publishes nothing.

On a later round the reviewer settles each thread it opened before it
hunts for more. The author answers in one of three ways: `Fixed in <sha>:
...`, `Not changed: <why>.`, or a plain answer to a question. Once the
defect is actually gone at the head, the reviewer resolves the thread without
a reply. In a supervised repository, the reviewer leaves it for the human.
Where the
last reply claims a fix (`Fixed in <sha>: ...`) but the defect is still
there, the reviewer replies once, `Still there: <why>.`, and never says it
again in that thread. Any other answered reply that leaves the defect in
place — a `Not changed:` reply, or any other — gets no reply back and the thread stays open for the user to settle. A
thread nobody has answered gets nothing: it waits. On a request a task's
agent keeps, the review's findings go to that agent like anyone else's
comments, though they are posted under your login: it answers each one,
and fixes what needs fixing.

## What a `pr-babysit` keeper session does

A request of yours that no task opened can be kept by hand: call `PUT
/v1/repositories/<repo-id>/pull-requests/<number>/ariadne-babysit` with
`{"asked": true, "model": "<agent:model>", "effort": "<effort>", "skills":
[...]}`, from the CLI as `ariadne pr babysit <repo-id> <number> --model
<agent:model>`. `--model` is required, the same way `pr review`'s is. A
request a task opened, or one by another login, cannot be babysat this
way: asking is refused with 409.

Ask the same way from the desktop app: open a request of yours that no
task keeps in the Pull requests tab and press **Start babysit** in its
panel, beside Start review. The dialog asks the model and the effort the
keeper runs on, and which skills it loads beside `pr-babysit`, its own
playbook, always on; any other skill can join it. Starting it closes the
dialog and, once the daemon has the keeper session up, opens that
session's console in the panel, the same way starting a review does. An
**Ariadne babysitting** pill marks the row and the panel while one runs,
beside Ariadne reviewing. **Stop babysit** takes it down. The panel's
Sessions tab lists the keeper session beside a review's.

The keeper session loads `pr-babysit` — the same skill a task's `pr`
column stages — and any other skill you pick. Its worktree is a local
branch of its own, `ariadne/keep-<id>`, cut from the request's head, never
a detached checkout: the keeper commits a fix there and pushes it onto the
request's own head branch. The same refusal a reviewer's own spawn makes
holds here too: a head branch git itself refuses as a ref name starts no
worktree and no keeper session. It hears the same news a task's keeping agent
does — a new comment, a check that turned to failure or back to green, a
changed review decision, a merge or a close — replies and fixes the same
way, and reports the request ready the same way. Unlike a self-review,
which only ends when you stop asking, babysitting also ends on the merge
or the close: the session, its worktree and its branch all go.
`ariadne pr babysit <repo-id> <number> --stop` takes it down early.

## Nothing approves or merges in your name

No session can post an approval or merge a request — there is no tool for
either. `pr-reviewer` posts `request_changes` or `comment`, never an
approval; the approving call, and the merge, are always yours. The one
thread a session resolves is a review thread it opened itself: on a later
round, once a push fixed that finding, it replies and resolves it. Every
thread anyone else opened stays yours to resolve.

## Needs attention: an unassigned review and a confirmed-ready request

Neither a posted review nor a `pr` agent's own ready report notifies you by
itself: a `pr-reviewer` session finishing its review raises nothing, and a
`ready` report is a claim, not confirmed evidence. Two things reach Needs
attention instead, in `ariadne attention` and the desktop app's attention
strip, each naming the request's title, its repository, the one action that
clears it and how long it has waited, and each linking to the request's own
panel:

- **Review needed** — a request that asks for your review and has no
  review asked yet: see the previous section for starting one by hand.
- **Ready to merge** — a request a `pr` agent keeps, or one a keeper
  session babysits by hand, once its own `ready` report is actually
  backed up by the forge's own evidence for the request's current head: a
  current approval, every review thread actually resolved — a reply alone
  is not enough — every check green, and the forge's own confirmation the
  head can be merged now. A later push, a reopened comment or an approval
  the forge no longer counts drops the item until the keeping agent
  reports ready again, on the new head. The merge itself is still always
  yours: Ariadne gives no approval.

## Webhooks: a delivery wakes the daemon, the daemon wakes the agent

Every daemon runs a webhook listener, bound to `webhook_listen` (default a
random free loopback port) and separate from the REST API: it serves only
`POST /webhooks/github/<repository-id>`, and nothing else — no `/v1`, no
documentation. Each delivery is checked against the integration's own
secret, constant-time, with GitHub's `X-Hub-Signature-256`. A missing or
wrong one gets 401 and triggers nothing. An authenticated delivery gets
202; one that is not a connectivity-check `ping` wakes one fetch of the
repository — it never applies the delivery's payload directly.

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

A hook that is `live` has no timer: a delivery that never arrives there
waits on the next wake, whatever triggers it — another delivery, or
`ariadne pr refresh` (the desktop app's own Refresh button is gone while
every repository in view is live; see below). A tunnel that is down, or a
hook the forge refuses, moves the integration to `polling` or `failed`, and
there a missed delivery costs only the wait for the next timer fetch.
`repo inspect` prints the hook's `state`, `url`, `error`, the time of the
last delivery and the last fetch failure under the forge block; the
repositories table shows them as one pill, below.

Each fetch reads every open request of the repository, and which of them
ask for your review, and the open issues too: where the issues moved since
the last read, the desktop app reads them again on its own.

Ariadne never asks one forge host for more than two fetches at once: every
repository on the same host — `github.com`, or a GitHub Enterprise host —
shares that cap, whether its fetch started at startup,
on the five-minute timer, or on an explicit wake. A repository whose fetch
would push past the cap simply waits its turn, and runs the moment one of
the other two finishes, fails, or is cancelled by turning its integration
off mid-fetch; a repository on a different host is never held up by it.
This traffic limit is independent of each repository's own webhook state
and polling traffic above.

## The tunnel

With no `webhook_public_url` configured, Ariadne can open its own tunnel so
a forge on the public internet reaches your webhook listener without one —
the `localtunnel-client` crate, no Node or `lt` binary needed.

A brand-new installation starts with the switch off: every enabled
repository polls on its five-minute timer until you turn the tunnel on
yourself. Upgrading to this release turns it off too, once, even where it
was already on; after that one-time move, the switch is yours again, and
every later restart leaves your choice alone.

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
decision as pills, "by you" on a request of yours, **Ariadne reviewing**
while Ariadne reviews one of yours, and **Ariadne babysitting** while a
keeper session babysits one of yours by hand. A row holds no button: a
click on it opens the request's panel — its state and checks, its facts with
links to the forge and to each failed check, its description rendered, and
its sessions — where **Start review** and **Stop review** are, and, on a
request of yours that no task keeps, **Start babysit** and **Stop
babysit**. The list uses each repository's last fetch. Opening a request
reads its panel from the forge. Both update as a fetch finds a change. While
a repository in view polls, or its webhook is down, **Refresh** asks for a
fetch now; with every webhook live there is nothing to refresh, and the
button is gone.

```sh
ariadne pr ls --repo <repo-id>
ariadne pr ls --mine
ariadne pr ls --review-requests
ariadne pr inspect <repo-id> 42
```

`ariadne pr ls` lists every open request, `--mine` your own and
`--review-requests` the ones that ask for your review, as the tab's
three choices do, and `ariadne pr inspect` reads one whole.

## What Ariadne keeps of a request

Ariadne stores no forge content permanently. It holds titles, descriptions,
branches, checks and comments in memory from a fetch, and discards them on a
restart. Request lists use that last fetch; opening a request and the agent
tools `get_pull_request`, `list_comments` and `get_comment` read the forge at
the call.

It keeps a small record of a request only while it works on it: the request
a task opened, a request that asks for your review, and one of yours you
asked it to review or babysit. The record is
its own bookkeeping — which task keeps the request, what each agent was
told, the head last reviewed and a mark per comment it told or its review posted. Once the request merges or closes
and its work is taken down, the record goes, so an ended request takes no
room. The sessions that ran on it stay, with their history and spend.

Nothing adds or removes a request by hand, in the desktop app or the CLI:
it all happens on its own.

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
`gh` is not installed, or not signed in to the remote's host.
Run `gh auth login` for that host, then enable again;
the refusal carries `gh`'s own message, not a generic one.

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
