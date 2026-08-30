/**
 * The one query-key convention for the whole app.
 *
 * Every key is `[entity, "list" | "detail", ...]`:
 *
 *     ["goals", "list", filters?]            list of goals
 *     ["goals", "detail", id]                one goal
 *     ["tasks", "detail", id, "reviews"]     a sub-resource of that task
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

export interface PageFilters {
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

export const qk = {
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
    reviews: (id: string) => ["tasks", "detail", id, "reviews"] as const,
    transitions: (id: string) => ["tasks", "detail", id, "transitions"] as const,
    diff: (id: string) => ["tasks", "detail", id, "diff"] as const,
  },
  sessions: {
    all: () => ["sessions"] as const,
    lists: () => ["sessions", "list"] as const,
    list: (filters?: SessionFilters) => ["sessions", "list", filters ?? {}] as const,
    details: () => ["sessions", "detail"] as const,
    detail: (id: string) => ["sessions", "detail", id] as const,
    logs: (id: string) => ["sessions", "detail", id, "logs"] as const,
  },
  profiles: {
    all: () => ["profiles"] as const,
    lists: () => ["profiles", "list"] as const,
    list: (filters?: PageFilters) => ["profiles", "list", filters ?? {}] as const,
    details: () => ["profiles", "detail"] as const,
    detail: (id: string) => ["profiles", "detail", id] as const,
    prompts: (id: string) => ["profiles", "detail", id, "prompts"] as const,
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
   * How each coding-agent CLI is launched (`GET /v1/agents`): one unfiltered
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
  /** Raw hook-reported agent events (`GET /v1/events`). */
  agentEvents: {
    all: () => ["agent-events"] as const,
    lists: () => ["agent-events", "list"] as const,
    list: (filters?: AgentEventFilters) => ["agent-events", "list", filters ?? {}] as const,
  },
} as const
