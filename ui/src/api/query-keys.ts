/**
 * The one query-key convention for the whole app.
 *
 * Every key is `[entity, "list" | "detail", ...]`:
 *
 *     ["goals", "list", filters?]            list of goals
 *     ["goals", "detail", id]                one goal
 *     ["tasks", "detail", id, "messages"]    a sub-resource of that task
 *
 * Two consequences the SSE dispatcher relies on, so do not deviate:
 *
 * - invalidating `qk.goals.lists()` refetches every goal list without touching
 *   any open detail view;
 * - invalidating `qk.goals.detail(id)` also invalidates that goal's
 *   sub-resources, because they are nested under it.
 *
 * Filter objects must be plain and stable — TanStack Query hashes them
 * structurally, so `{ goal: id }` and `{ goal: id, status: undefined }` are the
 * same key.
 */

import type { SessionStatus, TaskStatus } from "./types"

interface PageFilters {
  after?: string
  limit?: number
}

interface TaskFilters extends PageFilters {
  goal?: string
  status?: TaskStatus
}

interface SessionFilters extends PageFilters {
  goal?: string
  task?: string
  status?: SessionStatus
}

interface AgentEventFilters extends PageFilters {
  session?: string
  task?: string
}

interface LearnedPermissionFilters {
  repository?: string
}

/**
 * What every `GET /v1/stats/<family>` is narrowed by: `since` is a span back
 * from now (`24h`, `7d`, `30d`), and `repo` a repository id.
 */
export interface StatsFilter {
  since?: string
  repo?: string
}

/** What `GET /v1/outside-sessions` narrows its snapshot by, page aside. */
interface OutsideSessionFilters {
  agent?: string
  dir?: string
  since?: string
  until?: string
  q?: string
}

export const qk = {
  pullRequests: {
    lists: () => ["pull-requests", "list"] as const,
    list: (filters?: { repo?: string; role?: string; state?: string }) =>
      ["pull-requests", "list", filters ?? {}] as const,
    detail: (id: string) => ["pull-requests", "detail", id] as const,
    search: (repo: string, q: string) => ["pull-requests", "search", { repo, q }] as const,
  },
  issues: {
    list: (repository: string, assigned: "me" | "all") =>
      ["issues", "list", { repository, assigned }] as const,
  },
  goals: {
    all: () => ["goals"] as const,
    lists: () => ["goals", "list"] as const,
    list: (filters?: PageFilters) => ["goals", "list", filters ?? {}] as const,
    details: () => ["goals", "detail"] as const,
    detail: (id: string) => ["goals", "detail", id] as const,
  },
  tasks: {
    all: () => ["tasks"] as const,
    lists: () => ["tasks", "list"] as const,
    list: (filters?: TaskFilters) => ["tasks", "list", filters ?? {}] as const,
    details: () => ["tasks", "detail"] as const,
    detail: (id: string) => ["tasks", "detail", id] as const,
    messages: (id: string) => ["tasks", "detail", id, "messages"] as const,
    transitions: (id: string) => ["tasks", "detail", id, "transitions"] as const,
    diff: (id: string) => ["tasks", "detail", id, "diff"] as const,
  },
  sessions: {
    all: () => ["sessions"] as const,
    lists: () => ["sessions", "list"] as const,
    list: (filters?: SessionFilters) => ["sessions", "list", filters ?? {}] as const,
    details: () => ["sessions", "detail"] as const,
    detail: (id: string) => ["sessions", "detail", id] as const,
  },
  /**
   * Sessions an ACP agent stored outside Ariadne (`GET /v1/outside-sessions`).
   * The cursor is not part of the key: the pages of one filter are the pages of
   * one infinite query, and it is that query's own page parameter.
   */
  outsideSessions: {
    all: () => ["outside-sessions"] as const,
    lists: () => ["outside-sessions", "list"] as const,
    list: (filters?: OutsideSessionFilters) => ["outside-sessions", "list", filters ?? {}] as const,
  },
  skills: {
    all: () => ["skills"] as const,
    lists: () => ["skills", "list"] as const,
    list: (filters?: PageFilters) => ["skills", "list", filters ?? {}] as const,
    details: () => ["skills", "detail"] as const,
    detail: (name: string) => ["skills", "detail", name] as const,
  },
  /**
   * The registered checkouts goals are created against
   * (`GET /v1/repositories`). One unfiltered list — the daemon takes no
   * filters — so `list()` is always keyed by the empty filter object.
   */
  repositories: {
    all: () => ["repositories"] as const,
    lists: () => ["repositories", "list"] as const,
    list: (filters?: PageFilters) => ["repositories", "list", filters ?? {}] as const,
    details: () => ["repositories", "detail"] as const,
    detail: (id: string) => ["repositories", "detail", id] as const,
  },
  /**
   * How each registry agent is launched (`GET /v1/agents`): one unfiltered
   * list of every agent kind, because the daemon answers with all of them.
   */
  agents: {
    all: () => ["agents"] as const,
    lists: () => ["agents", "list"] as const,
    list: () => ["agents", "list", {}] as const,
  },
  /** The model catalog (`GET /v1/models`), one unfiltered list for all agents. */
  models: {
    all: () => ["models"] as const,
    lists: () => ["models", "list"] as const,
    list: () => ["models", "list", {}] as const,
  },
  /**
   * The ACP agent registry and its cached discovery result
   * (`GET /v1/acp-agents`), one unfiltered list.
   */
  acpAgents: {
    all: () => ["acp-agents"] as const,
    lists: () => ["acp-agents", "list"] as const,
    list: () => ["acp-agents", "list", {}] as const,
  },
  /** Raw agent events the ACP runtime recorded (`GET /v1/events`). */
  agentEvents: {
    all: () => ["agent-events"] as const,
    lists: () => ["agent-events", "list"] as const,
    list: (filters?: AgentEventFilters) => ["agent-events", "list", filters ?? {}] as const,
  },
  /**
   * The AI permission model's settings, behind the `ai` permission mode
   * (`GET /v1/permissions/ai`). One row, so a detail key and no list.
   */
  permissions: {
    ai: () => ["permissions", "detail", "ai"] as const,
  },
  /**
   * The webhook tunnel's switch and state (`GET /v1/forge/tunnel`). One row,
   * so a detail key and no list.
   */
  forge: {
    tunnel: () => ["forge", "detail", "tunnel"] as const,
  },
  /**
   * The aggregates of the stats ledger (`GET /v1/stats/<family>`), one list
   * per family under one group: every family moves when a task or a session
   * does, so the dispatcher invalidates `all()`.
   */
  stats: {
    all: () => ["stats"] as const,
    work: (filter: StatsFilter) => ["stats", "list", "work", filter] as const,
    time: (filter: StatsFilter) => ["stats", "list", "time", filter] as const,
    spend: (filter: StatsFilter) => ["stats", "list", "spend", filter] as const,
    models: (filter: StatsFilter) => ["stats", "list", "models", filter] as const,
    attention: (filter: StatsFilter) => ["stats", "list", "attention", filter] as const,
  },
  learnedPermissions: {
    all: () => ["learned-permissions"] as const,
    lists: () => ["learned-permissions", "list"] as const,
    list: (filters?: LearnedPermissionFilters) =>
      ["learned-permissions", "list", filters ?? {}] as const,
    details: () => ["learned-permissions", "detail"] as const,
    detail: (id: string) => ["learned-permissions", "detail", id] as const,
  },
} as const
