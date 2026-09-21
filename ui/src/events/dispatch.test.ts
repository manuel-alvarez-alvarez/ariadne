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
  type MemoryDto,
  qk,
  type RepositoryDto,
  type TaskDto,
} from "@/api"
import { aGoal, aMemory, aRepository, aTask } from "@/test/fixtures"
import { dispatchDomainEvent, invalidateEverything } from "./dispatch"

const REPOSITORY: RepositoryDto = aRepository({
  id: "01JREPO00000000000000ARI",
  description: null,
})

const MEMORY: MemoryDto = aMemory({ repository_id: REPOSITORY.id })

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

describe("memory events", () => {
  it("refetches the list a saved memory belongs to", () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(qk.memories.list({ repository: REPOSITORY.id }), [])

    dispatch(queryClient, { event: "memory_created", data: MEMORY })

    expect(stale(queryClient, qk.memories.list({ repository: REPOSITORY.id }))).toBe(true)
  })

  it("refetches the list a deleted memory belonged to", () => {
    const queryClient = new QueryClient()
    queryClient.setQueryData(qk.memories.list({ repository: REPOSITORY.id }), [MEMORY])

    dispatch(queryClient, { event: "memory_deleted", data: { id: MEMORY.id } })

    expect(stale(queryClient, qk.memories.list({ repository: REPOSITORY.id }))).toBe(true)
  })

  it("refetches every list that holds a saved global memory", () => {
    const queryClient = globalMemoryLists()

    dispatch(queryClient, { event: "memory_created", data: aMemory({ repository_id: null }) })

    for (const key of GLOBAL_MEMORY_LISTS) expect(stale(queryClient, key)).toBe(true)
  })

  it("refetches every list that held a deleted global memory", () => {
    const queryClient = globalMemoryLists()

    dispatch(queryClient, {
      event: "memory_deleted",
      data: { id: MEMORY.id, repository_id: null },
    })

    for (const key of GLOBAL_MEMORY_LISTS) expect(stale(queryClient, key)).toBe(true)
  })
})

/** The lists a global memory is in: its own scope, every scope, a repository's page. */
const GLOBAL_MEMORY_LISTS = [
  qk.memories.list({ scope: "global" }),
  qk.memories.list({}),
  qk.memories.list({ repository: REPOSITORY.id }),
]

function globalMemoryLists(): QueryClient {
  const queryClient = new QueryClient()
  for (const key of GLOBAL_MEMORY_LISTS) queryClient.setQueryData(key, [])
  return queryClient
}

describe("knowledge events (022)", () => {
  /** A client showing the knowledge page, next to everything else. */
  function withKnowledge(): QueryClient {
    const queryClient = seeded()
    queryClient.setQueryData(qk.repositories.knowledgeStatus(REPOSITORY.id), {
      repository_id: REPOSITORY.id,
      state: "indexing",
      refs: [],
      files: 0,
      symbols: 0,
      languages: [],
      error: null,
    })
    queryClient.setQueryData(
      qk.repositories.knowledgeInteractions(REPOSITORY.id, { repository: REPOSITORY.id }),
      [],
    )
    queryClient.setQueryData(
      qk.repositories.knowledgeImpact(REPOSITORY.id, {
        repository: REPOSITORY.id,
        git_ref: "main",
        symbol: "parse",
        depth: 2,
      }),
      [],
    )
    queryClient.setQueryData(
      qk.repositories.knowledgePath(REPOSITORY.id, {
        repository: REPOSITORY.id,
        git_ref: "main",
        from: "a",
        to: "b",
        depth: 6,
      }),
      { hops: [] },
    )
    return queryClient
  }

  /** Every impact and path walk `withKnowledge` seeded. */
  function walksAreStale(queryClient: QueryClient): boolean {
    return queryClient
      .getQueryCache()
      .findAll({ queryKey: qk.repositories.knowledgeWalksAll(REPOSITORY.id) })
      .every((query) => query.state.isInvalidated)
  }

  it("refetches the status and every interactions list once indexing finishes", () => {
    const queryClient = withKnowledge()

    dispatch(queryClient, {
      event: "knowledge_indexed",
      data: {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        commit: "abc1230000000000000000000000000000000000",
        files: 12,
        symbols: 34,
      },
    })

    expect(stale(queryClient, qk.repositories.knowledgeStatus(REPOSITORY.id))).toBe(true)
    expect(
      stale(
        queryClient,
        qk.repositories.knowledgeInteractions(REPOSITORY.id, { repository: REPOSITORY.id }),
      ),
    ).toBe(true)
  })

  it("refetches every impact and path walk once indexing finishes, and when it fails", () => {
    const finished = withKnowledge()
    const failed = withKnowledge()

    dispatch(finished, {
      event: "knowledge_indexed",
      data: {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        commit: "abc1230000000000000000000000000000000000",
        files: 12,
        symbols: 34,
      },
    })
    dispatch(failed, {
      event: "knowledge_failed",
      data: {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        error: "git clone failed: permission denied",
      },
    })

    expect(
      finished
        .getQueryCache()
        .findAll({ queryKey: qk.repositories.knowledgeWalksAll(REPOSITORY.id) }),
    ).toHaveLength(2)
    expect(walksAreStale(finished)).toBe(true)
    expect(walksAreStale(failed)).toBe(true)
  })

  it("refetches the status and every interactions list when indexing fails", () => {
    const queryClient = withKnowledge()

    dispatch(queryClient, {
      event: "knowledge_failed",
      data: {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        error: "git clone failed: permission denied",
      },
    })

    expect(stale(queryClient, qk.repositories.knowledgeStatus(REPOSITORY.id))).toBe(true)
    expect(
      stale(
        queryClient,
        qk.repositories.knowledgeInteractions(REPOSITORY.id, { repository: REPOSITORY.id }),
      ),
    ).toBe(true)
  })

  it("refetches every file graph and outline once indexing finishes or fails", () => {
    const graphKey = qk.repositories.knowledgeGraph(REPOSITORY.id, {
      repository: REPOSITORY.id,
      git_ref: "main",
      limit: 4000,
    })
    const outlineKey = qk.repositories.knowledgeOutline(REPOSITORY.id, {
      repository: REPOSITORY.id,
      path: "src/app.ts",
      git_ref: "main",
    })
    for (const event of [
      {
        event: "knowledge_indexed" as const,
        data: {
          repository_id: REPOSITORY.id,
          git_ref: "main",
          commit: "abc1230000000000000000000000000000000000",
          files: 12,
          symbols: 34,
        },
      },
      {
        event: "knowledge_failed" as const,
        data: { repository_id: REPOSITORY.id, git_ref: "main", error: "no" },
      },
    ]) {
      const queryClient = withKnowledge()
      queryClient.setQueryData(graphKey, { nodes: [], edges: [] })
      queryClient.setQueryData(outlineKey, [])

      dispatch(queryClient, event)

      expect(stale(queryClient, graphKey)).toBe(true)
      expect(stale(queryClient, outlineKey)).toBe(true)
    }
  })

  it("leaves another repository's knowledge caches alone", () => {
    const queryClient = withKnowledge()
    const other = "01JREPO000000000000OTHER1"
    queryClient.setQueryData(qk.repositories.knowledgeStatus(other), {
      repository_id: other,
      state: "idle",
      refs: [],
      files: 0,
      symbols: 0,
      languages: [],
      error: null,
    })

    dispatch(queryClient, {
      event: "knowledge_indexed",
      data: {
        repository_id: REPOSITORY.id,
        git_ref: "main",
        commit: "abc1230000000000000000000000000000000000",
        files: 12,
        symbols: 34,
      },
    })

    expect(stale(queryClient, qk.repositories.knowledgeStatus(other))).toBe(false)
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
