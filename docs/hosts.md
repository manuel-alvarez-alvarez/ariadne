# Sending tasks to other hosts

A host is another machine that runs its own Ariadne daemon. A goal can send
some of its tasks to a host. That host runs the agents of the task with its own
repositories, its own agents and its own models.

Add and set up a host first. See [Using the CLI](cli.md) for `ariadne host`.
The rules are in [the host specification](../specs/037-hosts.md).

## How the orchestrator chooses

While it plans, the orchestrator does these steps:

1. It calls `list_hosts`. Each host has a `name`, a `status`, a `sync.state`
   and the `sync.missing_repositories` of the host.
2. It picks one `ready` host for each task. It does not pick a host that lacks
   the repository of the task.
3. It calls `list_models` with the `host` of the task. It staffs the task only
   on models of that host.
4. It gives the `name` of the host as `host` on `create_task`.

A task with no host runs on this machine.

## Why the models are those of the host

Each host finds its own agents and models. A model of this machine can be
missing on a far host. For this reason, the orchestrator reads the models of
the host that runs the task.

You can read the same list yourself. The daemon serves it at
`GET /v1/hosts/{host}/models`:

- `local` answers the models of this daemon.
- An unknown host answers `404`.
- A host that is not `ready` answers `503`. The message names its status.

## What you do

- Check that each host is `ready` in `ariadne host ls` before you start a goal.
- Wait until `sync.state` is `in_sync`, or the host can lack a skill or a setting.
- Register the repository on the host if it is in `sync.missing_repositories`.
