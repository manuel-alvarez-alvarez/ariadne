/**
 * What the repository, goal-deletion and task events do to the query cache.
 *
 * This is what keeps a second window live: nothing on these screens polls, and
 * nothing on them handles events itself, so a repository registered, edited or
 * removed anywhere — another window, the CLI — only shows up because these
 * cases reach the keys the screen reads.
 *
 * Two of them reach outside their own entity, which is why they are asserted
 * twice over. `repository_updated`: a goal carries its repositories inline and
 * references them live, so an edited path is wrong in every goal that works in
 * it until the goals are read again. `goal_deleted`: the goal takes its tasks
 * and their sessions with it, and those are listed goal-first rather than
 * pruned entry by entry.
 *
 * `task_branch_updated` is the odd one the other way round: it is the only
 * event that says nothing about a row, so the assertions are as much about what
 * it leaves alone as about the diff it refetches.
 *
 * The recovery path is asserted against the client the app actually ships
 * ({@link createQueryClient}), because that is where it could go wrong: nothing
 * in it goes stale on its own any more, so a reconnect that only marked entries
 * stale would leave the screen showing what the daemon said before the gap.
 */

import { QueryClient, QueryObserver } from "@tanstack/react-query"
import { describe, expect, it, vi } from "vitest"

import {
  createQueryClient,
  type DomainEvent,
  type GoalDto,
  qk,
  type RepositoryDto,
  rememberHosts,
  type SessionDto,
  type TaskDto,
} from "@/api"
import {
  aDashboard,
  aGoal,
  aLearnedPermission,
  anAiPermissionsStatus,
  aRepository,
  aSession,
  aTask,
  aWorkflow,
} from "@/test/fixtures"
import { BOX, forgetHosts, LOCAL } from "@/test/hosts"
import { dispatchDomainEvent, invalidateEverything } from "./dispatch"

const REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
  description: null,
})

const GOAL: GoalDto = aGoal({
  status: "completed",
})

const TASK: TaskDto = aTask()

/** A client with a list and a detail already in it, as an open screen has. */
function seeded(): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  queryClient.setQueryData(qk.repositories.list("local"), [REPOSITORY])
  queryClient.setQueryData(qk.repositories.detail("local", REPOSITORY.id), REPOSITORY)
  queryClient.setQueryData(qk.goals.list("local"), [])
  return queryClient
}

/** Whether the entry under `key` was marked for refetching. */
function stale(queryClient: QueryClient, key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key)?.isInvalidated === true
}

function dispatch(queryClient: QueryClient, event: DomainEvent): void {
  dispatchDomainEvent(queryClient, "local", event)
}

describe("repository events", () => {
  it("writes a created repository into its detail and refetches the list", () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(qk.repositories.list("local"), [])

    dispatch(queryClient, { event: "repository_created", data: REPOSITORY })

    expect(queryClient.getQueryData(qk.repositories.detail("local", REPOSITORY.id))).toEqual(
      REPOSITORY,
    )
    expect(stale(queryClient, qk.repositories.list("local"))).toBe(true)
  })

  it("patches an edited repository in place", () => {
    const queryClient = seeded()
    const moved = { ...REPOSITORY, path: "/srv/ariadne", base_branch: "trunk" }

    dispatch(queryClient, { event: "repository_updated", data: moved })

    expect(queryClient.getQueryData(qk.repositories.detail("local", REPOSITORY.id))).toEqual(moved)
    expect(stale(queryClient, qk.repositories.list("local"))).toBe(true)
  })

  it("refetches the goals too, because they carry the repository they reference", () => {
    const queryClient = seeded()

    dispatch(queryClient, {
      event: "repository_updated",
      data: { ...REPOSITORY, path: "/srv/ariadne" },
    })

    expect(stale(queryClient, qk.goals.list("local"))).toBe(true)
  })

  it("drops a removed repository rather than leaving it in the cache", () => {
    const queryClient = seeded()

    dispatch(queryClient, { event: "repository_deleted", data: { id: REPOSITORY.id } })

    expect(queryClient.getQueryData(qk.repositories.detail("local", REPOSITORY.id))).toBeUndefined()
    expect(stale(queryClient, qk.repositories.list("local"))).toBe(true)
  })
})

describe("goal events", () => {
  it("drops a deleted goal, and refetches everything that hung off it", () => {
    const queryClient = seeded()
    queryClient.setQueryData(qk.goals.list("local"), [GOAL])
    queryClient.setQueryData(qk.goals.detail("local", GOAL.id), GOAL)
    queryClient.setQueryData(qk.tasks.list("local", { goal: GOAL.id }), [])
    queryClient.setQueryData(qk.sessions.list("local", { goal: GOAL.id }), [])

    dispatch(queryClient, { event: "goal_deleted", data: { id: GOAL.id } })

    // The board is what has to lose the row, without anyone touching it.
    expect(stale(queryClient, qk.goals.list("local"))).toBe(true)
    // Nothing of the goal is left to be read back out of the cache.
    expect(queryClient.getQueryData(qk.goals.detail("local", GOAL.id))).toBeUndefined()
    // Its tasks and their sessions were deleted with it.
    expect(stale(queryClient, qk.tasks.list("local", { goal: GOAL.id }))).toBe(true)
    expect(stale(queryClient, qk.sessions.list("local", { goal: GOAL.id }))).toBe(true)
  })
})

describe("task events", () => {
  /** A client showing the diff tab of a task, next to everything else. */
  function withDiff(): QueryClient {
    const queryClient = seeded()
    queryClient.setQueryData(qk.tasks.list("local"), [TASK])
    queryClient.setQueryData(qk.tasks.detail("local", TASK.id), TASK)
    queryClient.setQueryData(qk.tasks.diff("local", TASK.id), "diff --git a/one b/one\n")
    queryClient.setQueryData(qk.sessions.list("local"), [])
    return queryClient
  }

  it("refetches the diff when the task branch's head moves", () => {
    const queryClient = withDiff()

    dispatch(queryClient, {
      event: "task_branch_updated",
      data: {
        task_id: TASK.id,
        goal_id: TASK.goal_id,
        branch: TASK.branch,
        head: "1ea7fca11ab1e0000000000000000000000000de",
      },
    })

    expect(stale(queryClient, qk.tasks.diff("local", TASK.id))).toBe(true)
  })

  it("leaves every row alone: a commit changes the diff and nothing else", () => {
    const queryClient = withDiff()

    dispatch(queryClient, {
      event: "task_branch_updated",
      data: {
        task_id: TASK.id,
        goal_id: TASK.goal_id,
        branch: TASK.branch,
        head: "1ea7fca11ab1e0000000000000000000000000de",
      },
    })

    expect(queryClient.getQueryData(qk.tasks.detail("local", TASK.id))).toEqual(TASK)
    expect(stale(queryClient, qk.tasks.list("local"))).toBe(false)
    expect(stale(queryClient, qk.goals.list("local"))).toBe(false)
    expect(stale(queryClient, qk.sessions.list("local"))).toBe(false)
    expect(stale(queryClient, qk.repositories.list("local"))).toBe(false)
    expect(stale(queryClient, qk.repositories.detail("local", REPOSITORY.id))).toBe(false)
  })

  it("refetches the diff when the task lands, because it then diffs the merge commit", () => {
    const queryClient = withDiff()

    dispatch(queryClient, {
      event: "task_updated",
      data: {
        task: {
          ...TASK,
          status: "finished",
          merge_commit: "abc1230000000000000000000000000000000000",
        },
        transition: {
          id: "01JTRAN0000000000000000001",
          actor: "daemon",
          from_status: "approved",
          to_status: "finished",
          created_at: TASK.updated_at,
        },
      },
    })

    expect(stale(queryClient, qk.tasks.diff("local", TASK.id))).toBe(true)
  })

  it("holds on to the diff for an update that is not a transition", () => {
    const queryClient = withDiff()

    dispatch(queryClient, {
      event: "task_updated",
      data: { task: { ...TASK, title: "Renamed" }, transition: null },
    })

    expect(stale(queryClient, qk.tasks.diff("local", TASK.id))).toBe(false)
  })
})

describe("invalidateEverything", () => {
  it("refetches what is on screen after a gap in the stream", async () => {
    const queryClient = createQueryClient()
    const list = vi.fn().mockResolvedValue([REPOSITORY])
    const observer = new QueryObserver(queryClient, {
      queryKey: qk.repositories.list("local"),
      queryFn: list,
    })
    const unsubscribe = observer.subscribe(() => {})
    await vi.waitFor(() =>
      expect(queryClient.getQueryData(qk.repositories.list("local"))).toBeDefined(),
    )

    invalidateEverything(queryClient, "local")

    await vi.waitFor(() => expect(list).toHaveBeenCalledTimes(2))
    unsubscribe()
  })

  it("retries a daemon identity lookup that failed while the host was down", async () => {
    const queryClient = createQueryClient()
    const lookup = vi
      .fn()
      .mockRejectedValueOnce(new Error("down"))
      .mockResolvedValue({ protocol: "remote-tasks-v1", daemon_id: "d1" })
    const key = qk.remoteTaskProtocol("box", "")
    const observer = new QueryObserver(queryClient, {
      queryKey: key,
      queryFn: lookup,
      retry: false,
    })
    const unsubscribe = observer.subscribe(() => {})
    await vi.waitFor(() => expect(observer.getCurrentResult().isError).toBe(true))

    invalidateEverything(queryClient, "box")

    await vi.waitFor(() => expect(observer.getCurrentResult().data).toBeDefined())
    expect(lookup).toHaveBeenCalledTimes(2)
    unsubscribe()
  })

  it("marks what is off screen stale, so it is read again when it is next shown", () => {
    const queryClient = createQueryClient()
    queryClient.setQueryData(qk.goals.list("local"), [GOAL])

    invalidateEverything(queryClient, "local")

    expect(stale(queryClient, qk.goals.list("local"))).toBe(true)
  })
})

describe("pull request session events", () => {
  /** A client holding a pull request list and its detail, as its screen and panel do. */
  function withPullRequest(): QueryClient {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.pullRequests.list("local"), [])
    queryClient.setQueryData(qk.pullRequests.detail("local", "pull-42"), {})
    return queryClient
  }

  it("refetches the request a session watches when that session starts or moves, for its session_id", () => {
    for (const event of ["session_created", "session_updated"] as const) {
      const queryClient = withPullRequest()

      dispatch(queryClient, { event, data: aSession({ pull_request_id: "pull-42" }) })

      expect(stale(queryClient, qk.pullRequests.list("local"))).toBe(true)
      expect(stale(queryClient, qk.pullRequests.detail("local", "pull-42"))).toBe(true)
    }
  })

  it("reads every request again on pull_requests_changed, which carries none", () => {
    const queryClient = withPullRequest()
    queryClient.setQueryData(qk.pullRequests.detail("local", "repo:42"), {})

    dispatch(queryClient, { event: "pull_requests_changed", data: { repository_id: "repo" } })

    expect(stale(queryClient, qk.pullRequests.list("local"))).toBe(true)
    expect(stale(queryClient, qk.pullRequests.detail("local", "pull-42"))).toBe(true)
    expect(stale(queryClient, qk.pullRequests.detail("local", "repo:42"))).toBe(true)
  })

  it("leaves the pull requests alone for a session that watches none", () => {
    for (const event of ["session_created", "session_updated"] as const) {
      const queryClient = withPullRequest()

      dispatch(queryClient, { event, data: aSession() })

      expect(stale(queryClient, qk.pullRequests.list("local"))).toBe(false)
      expect(stale(queryClient, qk.pullRequests.detail("local", "pull-42"))).toBe(false)
    }
  })
})

describe("session events", () => {
  const OLDER = "2026-11-21T10:00:00Z"
  const NEWER = "2026-11-21T11:00:00Z"
  const NEWEST = "2026-11-21T12:00:00Z"

  /** The ids a cached sessions list holds, in order. */
  function ids(queryClient: QueryClient, filters: Record<string, unknown> = {}): string[] {
    const rows = queryClient.getQueryData<SessionDto[]>(qk.sessions.list("local", filters))
    return (rows ?? []).map((row) => row.id)
  }

  it("patches a moved session into every cached list and refetches none of them", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const a = aSession({ id: "a", task_id: "task", last_activity_at: NEWER })
    const b = aSession({ id: "b", task_id: "task", last_activity_at: OLDER })
    queryClient.setQueryData(qk.sessions.list("local", {}), [a, b])
    queryClient.setQueryData(qk.sessions.list("local", { task: "task" }), [a, b])

    dispatch(queryClient, {
      event: "session_updated",
      data: { ...b, status: "idle", last_activity_at: NEWEST },
    })

    for (const filters of [{}, { task: "task" }]) {
      expect(stale(queryClient, qk.sessions.list("local", filters))).toBe(false)
      expect(ids(queryClient, filters)).toEqual(["b", "a"])
    }
    const rows = queryClient.getQueryData<SessionDto[]>(qk.sessions.list("local", {}))
    expect(rows?.[0]?.status).toBe("idle")
  })

  it("moves a session between status lists and leaves another goal's list alone", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const session = aSession({ id: "a", status: "running" })
    const other = [aSession({ id: "z", goal_id: "another-goal" })]
    queryClient.setQueryData(qk.sessions.list("local", { status: "running" }), [session])
    queryClient.setQueryData(qk.sessions.list("local", { status: "exited" }), [])
    queryClient.setQueryData(qk.sessions.list("local", { goal: "another-goal" }), other)

    dispatch(queryClient, { event: "session_updated", data: { ...session, status: "exited" } })

    expect(ids(queryClient, { status: "running" })).toEqual([])
    expect(ids(queryClient, { status: "exited" })).toEqual(["a"])
    expect(queryClient.getQueryData(qk.sessions.list("local", { goal: "another-goal" }))).toBe(
      other,
    )
  })

  it("adds a new session to the lists it belongs in", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const old = aSession({ id: "a", last_activity_at: OLDER })
    queryClient.setQueryData(qk.sessions.list("local", {}), [old])

    dispatch(queryClient, {
      event: "session_created",
      data: aSession({ id: "b", last_activity_at: NEWEST }),
    })

    expect(stale(queryClient, qk.sessions.list("local", {}))).toBe(false)
    expect(ids(queryClient)).toEqual(["b", "a"])
  })

  it("refetches a list whose read is still under way, which a patch would lose to", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    let answer: (rows: SessionDto[]) => void = () => {}
    const pending = new Promise<SessionDto[]>((resolve) => {
      answer = resolve
    })
    const queryFn = vi.fn(() => pending)
    const reading = queryClient.fetchQuery({ queryKey: qk.sessions.list("local", {}), queryFn })

    dispatch(queryClient, { event: "session_updated", data: aSession({ status: "exited" }) })

    expect(stale(queryClient, qk.sessions.list("local", {}))).toBe(true)
    answer([aSession({ status: "running" })])
    await reading
  })
})

describe("ai permissions events", () => {
  it("replaces the cached status whole, so a card that read installing reads ready", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(
      qk.permissions.ai("local"),
      anAiPermissionsStatus({ state: "installing" }),
    )

    dispatch(queryClient, {
      event: "ai_permissions_updated",
      data: anAiPermissionsStatus({ state: "ready", installed_release: "v0.1.4" }),
    })

    expect(queryClient.getQueryData(qk.permissions.ai("local"))).toEqual(
      anAiPermissionsStatus({ state: "ready", installed_release: "v0.1.4" }),
    )
  })
})

describe("forge settings events", () => {
  it("replaces the cached tunnel whole, so a screen that read up reads off", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const up = {
      enabled: true,
      state: "up" as const,
      url: "https://smee.io/Tl13s4JZbHs6Ww60eXq7Ab",
      listen: "127.0.0.1:49152",
      since: "2026-10-08T10:00:00.000Z",
      error: null,
    }
    queryClient.setQueryData(qk.forge.tunnel("local"), up)
    const off = { ...up, enabled: false, state: "off" as const, url: null }

    dispatch(queryClient, { event: "forge_settings_updated", data: off })

    expect(queryClient.getQueryData(qk.forge.tunnel("local"))).toEqual(off)
  })
})

describe("issue events", () => {
  it("refetches both assignment filters of the repository whose issues moved, and no other", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    for (const key of [
      qk.issues.list("local", "repo-1", "me"),
      qk.issues.list("local", "repo-1", "all"),
      qk.issues.list("local", "repo-2", "me"),
    ]) {
      queryClient.setQueryData(key, [])
    }

    dispatch(queryClient, { event: "issues_changed", data: { repository_id: "repo-1" } })

    expect(stale(queryClient, qk.issues.list("local", "repo-1", "me"))).toBe(true)
    expect(stale(queryClient, qk.issues.list("local", "repo-1", "all"))).toBe(true)
    expect(stale(queryClient, qk.issues.list("local", "repo-2", "me"))).toBe(false)
  })
})

describe("learned permission events", () => {
  const learned = aLearnedPermission({
    repository_id: REPOSITORY.id,
  })

  it("patches created and updated details and refetches lists", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const list = qk.learnedPermissions.list("local", { repository: REPOSITORY.id })
    queryClient.setQueryData(list, { items: [] })
    dispatch(queryClient, { event: "learned_permission_created", data: learned })
    expect(queryClient.getQueryData(qk.learnedPermissions.detail("local", learned.id))).toEqual(
      learned,
    )
    expect(stale(queryClient, list)).toBe(true)

    const updated = { ...learned, tool_name: "Shell" }
    dispatch(queryClient, { event: "learned_permission_updated", data: updated })
    expect(queryClient.getQueryData(qk.learnedPermissions.detail("local", learned.id))).toEqual(
      updated,
    )
    expect(stale(queryClient, list)).toBe(true)
  })

  it("removes a deleted detail and refetches lists", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const list = qk.learnedPermissions.list("local", {})
    queryClient.setQueryData(list, { items: [learned] })
    queryClient.setQueryData(qk.learnedPermissions.detail("local", learned.id), learned)
    dispatch(queryClient, { event: "learned_permission_deleted", data: learned })
    expect(
      queryClient.getQueryData(qk.learnedPermissions.detail("local", learned.id)),
    ).toBeUndefined()
    expect(stale(queryClient, list)).toBe(true)
  })
})

describe("agent events", () => {
  it("refetches the session activity lists, since the frame carries no payload to apply", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const activity = qk.agentEvents.list("local", { session: "01JSESSION0000000000000ARI" })
    queryClient.setQueryData(activity, [])

    dispatch(queryClient, {
      event: "agent_event",
      data: {
        id: "01JEVENT000000000000000001",
        session_id: "01JSESSION0000000000000ARI",
        task_id: TASK.id,
        kind: "post_tool_use",
        summary: "Read AGENTS.md",
        created_at: "2026-09-20T00:00:00.000Z",
      },
    })

    expect(stale(queryClient, activity)).toBe(true)
  })
})

describe("recovery attention", () => {
  it("refetches the attention list on a goal's own confirmed-turn or give-up evidence", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.attention.list("local"), { items: [], complete: true })

    dispatch(queryClient, { event: "goal_updated", data: GOAL })

    expect(stale(queryClient, qk.attention.list("local"))).toBe(true)
  })

  it("refetches the attention list on a reviewer session's own give-up evidence", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.attention.list("local"), { items: [], complete: true })

    dispatch(queryClient, {
      event: "pull_requests_changed",
      data: { repository_id: "repo" },
    })

    expect(stale(queryClient, qk.attention.list("local"))).toBe(true)
  })
})

describe("workflow events", () => {
  it("patches created and updated workflows, then drops a deleted workflow", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const workflow = aWorkflow()
    queryClient.setQueryData(qk.workflows.list("local"), [workflow])

    dispatch(queryClient, { event: "workflow_created", data: workflow })
    expect(queryClient.getQueryData(qk.workflows.detail("local", workflow.name))).toEqual(workflow)
    expect(stale(queryClient, qk.workflows.list("local"))).toBe(true)

    const updated = { ...workflow, document: `${workflow.document}\n  done[Done]` }
    dispatch(queryClient, { event: "workflow_updated", data: updated })
    expect(queryClient.getQueryData(qk.workflows.detail("local", workflow.name))).toEqual(updated)

    dispatch(queryClient, { event: "workflow_deleted", data: { id: workflow.name } })
    expect(queryClient.getQueryData(qk.workflows.detail("local", workflow.name))).toBeUndefined()
  })
})

describe("dashboard events", () => {
  it("patches created and updated dashboards, then drops a deleted dashboard", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const dashboard = aDashboard()
    queryClient.setQueryData(qk.dashboards.list("local"), [dashboard])

    dispatch(queryClient, { event: "dashboard_created", data: dashboard })
    expect(queryClient.getQueryData(qk.dashboards.detail("local", dashboard.name))).toEqual(
      dashboard,
    )
    expect(stale(queryClient, qk.dashboards.list("local"))).toBe(true)

    const updated = { ...dashboard, document_is_default: false }
    dispatch(queryClient, { event: "dashboard_updated", data: updated })
    expect(queryClient.getQueryData(qk.dashboards.detail("local", dashboard.name))).toEqual(updated)

    dispatch(queryClient, { event: "dashboard_deleted", data: updated })
    expect(queryClient.getQueryData(qk.dashboards.detail("local", dashboard.name))).toBeUndefined()
  })
})

it("invalidates shared attention when another client dismisses an item", () => {
  const queryClient = new QueryClient()
  queryClient.setQueryData(qk.attention.list("local"), { items: [], complete: true })
  dispatch(queryClient, { event: "attention_changed", data: {} })
  expect(stale(queryClient, qk.attention.list("local"))).toBe(true)
})

describe("hosts", () => {
  it("patches and invalidates the keys of the host the event came from, and no other", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    for (const host of ["local", "box"]) {
      queryClient.setQueryData(qk.goals.detail(host, GOAL.id), GOAL)
      queryClient.setQueryData(qk.goals.list(host), [GOAL])
    }
    const renamed = { ...GOAL, title: "Renamed on box" }

    dispatchDomainEvent(queryClient, "box", { event: "goal_updated", data: renamed })

    expect(queryClient.getQueryData(qk.goals.detail("box", GOAL.id))).toEqual(renamed)
    expect(stale(queryClient, qk.goals.list("box"))).toBe(true)
    expect(queryClient.getQueryData(qk.goals.detail("local", GOAL.id))).toEqual(GOAL)
    expect(stale(queryClient, qk.goals.list("local"))).toBe(false)
  })

  it("refetches only the host whose stream had a gap", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.goals.list("local"), [])
    queryClient.setQueryData(qk.goals.list("box"), [])

    invalidateEverything(queryClient, "box")

    expect(stale(queryClient, qk.goals.list("box"))).toBe(true)
    expect(stale(queryClient, qk.goals.list("local"))).toBe(false)
  })

  it("patches the host's row on host_updated, and reads a host that came up again", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const down = { ...BOX, status: "down" as const, error: "connection refused" }
    queryClient.setQueryData(qk.hosts.list(), [LOCAL, down])
    rememberHosts([LOCAL, down])
    queryClient.setQueryData(qk.goals.list("box"), [])

    dispatch(queryClient, { event: "host_updated", data: BOX })

    expect(queryClient.getQueryData(qk.hosts.list())).toEqual([LOCAL, BOX])
    expect(stale(queryClient, qk.goals.list("box"))).toBe(true)
    forgetHosts()
  })

  it("ignores host_updated from a named host's own stream", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.hosts.list(), [LOCAL, BOX])

    dispatchDomainEvent(queryClient, "box", {
      event: "host_updated",
      data: { ...BOX, status: "down", error: "gone" },
    })

    expect(queryClient.getQueryData(qk.hosts.list())).toEqual([LOCAL, BOX])
  })
})

it("refreshes the source goal usage when a remote task changes", () => {
  const client = new QueryClient()
  client.setQueryData(qk.goals.detail("local", GOAL.id), GOAL)
  const task = aTask({
    goal_id: GOAL.id,
    execution_context: {
      source_goal: { daemon_id: "source", goal_id: GOAL.id, title: GOAL.title, description: "" },
      execution_owner: "worker",
      workflow: { name: "saved", steps: [] },
      revision: 1,
      authorized: true,
      authorized_revision: 1,
    },
  })
  dispatch(client, { event: "task_updated", data: { task, transition: null } })
  expect(stale(client, qk.goals.detail("local", GOAL.id))).toBe(true)
})
