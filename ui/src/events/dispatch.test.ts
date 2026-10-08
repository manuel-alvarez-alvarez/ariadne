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
  type TaskDto,
} from "@/api"
import {
  aGoal,
  aLearnedPermission,
  anAiPermissionsStatus,
  aRepository,
  aSession,
  aTask,
} from "@/test/fixtures"
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
  queryClient.setQueryData(qk.repositories.list(), [REPOSITORY])
  queryClient.setQueryData(qk.repositories.detail(REPOSITORY.id), REPOSITORY)
  queryClient.setQueryData(qk.goals.list(), [])
  return queryClient
}

/** Whether the entry under `key` was marked for refetching. */
function stale(queryClient: QueryClient, key: readonly unknown[]): boolean {
  return queryClient.getQueryState(key)?.isInvalidated === true
}

function dispatch(queryClient: QueryClient, event: DomainEvent): void {
  dispatchDomainEvent(queryClient, event)
}

describe("repository events", () => {
  it("writes a created repository into its detail and refetches the list", () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(qk.repositories.list(), [])

    dispatch(queryClient, { event: "repository_created", data: REPOSITORY })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toEqual(REPOSITORY)
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })

  it("patches an edited repository in place", () => {
    const queryClient = seeded()
    const moved = { ...REPOSITORY, path: "/srv/ariadne", base_branch: "trunk" }

    dispatch(queryClient, { event: "repository_updated", data: moved })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toEqual(moved)
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })

  it("refetches the goals too, because they carry the repository they reference", () => {
    const queryClient = seeded()

    dispatch(queryClient, {
      event: "repository_updated",
      data: { ...REPOSITORY, path: "/srv/ariadne" },
    })

    expect(stale(queryClient, qk.goals.list())).toBe(true)
  })

  it("drops a removed repository rather than leaving it in the cache", () => {
    const queryClient = seeded()

    dispatch(queryClient, { event: "repository_deleted", data: { id: REPOSITORY.id } })

    expect(queryClient.getQueryData(qk.repositories.detail(REPOSITORY.id))).toBeUndefined()
    expect(stale(queryClient, qk.repositories.list())).toBe(true)
  })
})

describe("goal events", () => {
  it("drops a deleted goal, and refetches everything that hung off it", () => {
    const queryClient = seeded()
    queryClient.setQueryData(qk.goals.list(), [GOAL])
    queryClient.setQueryData(qk.goals.detail(GOAL.id), GOAL)
    queryClient.setQueryData(qk.tasks.list({ goal: GOAL.id }), [])
    queryClient.setQueryData(qk.sessions.list({ goal: GOAL.id }), [])

    dispatch(queryClient, { event: "goal_deleted", data: { id: GOAL.id } })

    // The board is what has to lose the row, without anyone touching it.
    expect(stale(queryClient, qk.goals.list())).toBe(true)
    // Nothing of the goal is left to be read back out of the cache.
    expect(queryClient.getQueryData(qk.goals.detail(GOAL.id))).toBeUndefined()
    // Its tasks and their sessions were deleted with it.
    expect(stale(queryClient, qk.tasks.list({ goal: GOAL.id }))).toBe(true)
    expect(stale(queryClient, qk.sessions.list({ goal: GOAL.id }))).toBe(true)
  })
})

describe("task events", () => {
  /** A client showing the diff tab of a task, next to everything else. */
  function withDiff(): QueryClient {
    const queryClient = seeded()
    queryClient.setQueryData(qk.tasks.list(), [TASK])
    queryClient.setQueryData(qk.tasks.detail(TASK.id), TASK)
    queryClient.setQueryData(qk.tasks.diff(TASK.id), "diff --git a/one b/one\n")
    queryClient.setQueryData(qk.sessions.list(), [])
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

    expect(stale(queryClient, qk.tasks.diff(TASK.id))).toBe(true)
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

    expect(queryClient.getQueryData(qk.tasks.detail(TASK.id))).toEqual(TASK)
    expect(stale(queryClient, qk.tasks.list())).toBe(false)
    expect(stale(queryClient, qk.goals.list())).toBe(false)
    expect(stale(queryClient, qk.sessions.list())).toBe(false)
    expect(stale(queryClient, qk.repositories.list())).toBe(false)
    expect(stale(queryClient, qk.repositories.detail(REPOSITORY.id))).toBe(false)
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

    expect(stale(queryClient, qk.tasks.diff(TASK.id))).toBe(true)
  })

  it("holds on to the diff for an update that is not a transition", () => {
    const queryClient = withDiff()

    dispatch(queryClient, {
      event: "task_updated",
      data: { task: { ...TASK, title: "Renamed" }, transition: null },
    })

    expect(stale(queryClient, qk.tasks.diff(TASK.id))).toBe(false)
  })
})

describe("invalidateEverything", () => {
  it("refetches what is on screen after a gap in the stream", async () => {
    const queryClient = createQueryClient()
    const list = vi.fn().mockResolvedValue([REPOSITORY])
    const observer = new QueryObserver(queryClient, {
      queryKey: qk.repositories.list(),
      queryFn: list,
    })
    const unsubscribe = observer.subscribe(() => {})
    await vi.waitFor(() => expect(queryClient.getQueryData(qk.repositories.list())).toBeDefined())

    invalidateEverything(queryClient)

    await vi.waitFor(() => expect(list).toHaveBeenCalledTimes(2))
    unsubscribe()
  })

  it("marks what is off screen stale, so it is read again when it is next shown", () => {
    const queryClient = createQueryClient()
    queryClient.setQueryData(qk.goals.list(), [GOAL])

    invalidateEverything(queryClient)

    expect(stale(queryClient, qk.goals.list())).toBe(true)
  })
})

describe("session events", () => {
  it("refetches the outside lists when a session is created, since it may hold one of their rows", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const outside = qk.outsideSessions.list({})
    queryClient.setQueryData(outside, { pages: [], pageParams: [] })

    dispatch(queryClient, { event: "session_created", data: aSession() })

    expect(stale(queryClient, outside)).toBe(true)
  })

  it("leaves the outside lists alone when a session only moves on", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const outside = qk.outsideSessions.list({})
    queryClient.setQueryData(outside, { pages: [], pageParams: [] })

    dispatch(queryClient, { event: "session_updated", data: aSession() })

    expect(stale(queryClient, outside)).toBe(false)
  })
})

describe("pull request session events", () => {
  /** A client holding a pull request list and its detail, as its screen and panel do. */
  function withPullRequest(): QueryClient {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.pullRequests.list(), [])
    queryClient.setQueryData(qk.pullRequests.detail("pull-42"), {})
    return queryClient
  }

  it("refetches the request a session watches when that session starts or moves, for its session_id", () => {
    for (const event of ["session_created", "session_updated"] as const) {
      const queryClient = withPullRequest()

      dispatch(queryClient, { event, data: aSession({ pull_request_id: "pull-42" }) })

      expect(stale(queryClient, qk.pullRequests.list())).toBe(true)
      expect(stale(queryClient, qk.pullRequests.detail("pull-42"))).toBe(true)
    }
  })

  it("leaves the pull requests alone for a session that watches none", () => {
    for (const event of ["session_created", "session_updated"] as const) {
      const queryClient = withPullRequest()

      dispatch(queryClient, { event, data: aSession() })

      expect(stale(queryClient, qk.pullRequests.list())).toBe(false)
      expect(stale(queryClient, qk.pullRequests.detail("pull-42"))).toBe(false)
    }
  })
})

describe("ai permissions events", () => {
  it("replaces the cached status whole, so a card that read installing reads ready", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    queryClient.setQueryData(qk.permissions.ai(), anAiPermissionsStatus({ state: "installing" }))

    dispatch(queryClient, {
      event: "ai_permissions_updated",
      data: anAiPermissionsStatus({ state: "ready", installed_release: "v0.1.4" }),
    })

    expect(queryClient.getQueryData(qk.permissions.ai())).toEqual(
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
      url: "https://amber-104233.loca.lt",
      listen: "127.0.0.1:49152",
      since: "2026-10-08T10:00:00.000Z",
      error: null,
    }
    queryClient.setQueryData(qk.forge.tunnel(), up)
    const off = { ...up, enabled: false, state: "off" as const, url: null }

    dispatch(queryClient, { event: "forge_settings_updated", data: off })

    expect(queryClient.getQueryData(qk.forge.tunnel())).toEqual(off)
  })
})

describe("learned permission events", () => {
  const learned = aLearnedPermission({
    repository_id: REPOSITORY.id,
  })

  it("patches created and updated details and refetches lists", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const list = qk.learnedPermissions.list({ repository: REPOSITORY.id })
    queryClient.setQueryData(list, { items: [] })
    dispatch(queryClient, { event: "learned_permission_created", data: learned })
    expect(queryClient.getQueryData(qk.learnedPermissions.detail(learned.id))).toEqual(learned)
    expect(stale(queryClient, list)).toBe(true)

    const updated = { ...learned, tool_name: "Shell" }
    dispatch(queryClient, { event: "learned_permission_updated", data: updated })
    expect(queryClient.getQueryData(qk.learnedPermissions.detail(learned.id))).toEqual(updated)
    expect(stale(queryClient, list)).toBe(true)
  })

  it("removes a deleted detail and refetches lists", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const list = qk.learnedPermissions.list({})
    queryClient.setQueryData(list, { items: [learned] })
    queryClient.setQueryData(qk.learnedPermissions.detail(learned.id), learned)
    dispatch(queryClient, { event: "learned_permission_deleted", data: learned })
    expect(queryClient.getQueryData(qk.learnedPermissions.detail(learned.id))).toBeUndefined()
    expect(stale(queryClient, list)).toBe(true)
  })
})

describe("agent events", () => {
  it("refetches the session activity lists, since the frame carries no payload to apply", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const activity = qk.agentEvents.list({ session: "01JSESSION0000000000000ARI" })
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

describe("stats", () => {
  it("refetches every stat when a task, a session or a goal moves, since any may be a fact", () => {
    for (const event of [
      { event: "task_updated", data: { task: TASK } },
      { event: "session_updated", data: aSession() },
      { event: "goal_updated", data: GOAL },
    ] as DomainEvent[]) {
      const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
      const models = qk.stats.models({ since: "7d" })
      queryClient.setQueryData(models, { items: [] })

      dispatch(queryClient, event)

      expect(stale(queryClient, models)).toBe(true)
    }
  })
})
