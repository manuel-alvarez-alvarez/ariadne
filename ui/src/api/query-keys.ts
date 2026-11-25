/**
 * The one query-key convention for the whole app.
 *
 * Every key is `[host, entity, "list" | "detail", ...]`, the host first:
 *
 *     ["local", "goals", "list", filters?]            list of goals
 *     ["local", "goals", "detail", id]                one goal
 *     ["box", "tasks", "detail", id, "diff"]          a sub-resource of a task on `box`
 *
 * The host is the name `GET /v1/hosts` gives it, `local` for the settings
 * daemon. Ids are only unique on one daemon, so two hosts never share an
 * entry, and one host's whole cache is the prefix `qk.host(host)`.
 *
 * Two consequences the SSE dispatcher relies on, so do not deviate:
 *
 * - invalidating `qk.goals.lists(host)` refetches every goal list of that
 *   host without touching any open detail view or another host's lists;
 * - invalidating `qk.goals.detail(host, id)` also invalidates that goal's
 *   sub-resources, because they are nested under it.
 *
 * Filter objects must be plain and stable — TanStack Query hashes them
 * structurally, so `{ goal: id }` and `{ goal: id, status: undefined }` are the
 * same key.
 */

import { type Host, LOCAL_HOST } from "./client"
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

export const qk = {
  /** Every key of one host: what a gap in that host's stream invalidates. */
  host: (host: Host) => [host] as const,
  /**
   * The hosts the settings daemon is configured with (`GET /v1/hosts`). Read
   * from the settings daemon alone, so the key is always under `local`.
   */
  hosts: {
    update: (name: string) => [LOCAL_HOST, "hosts", "detail", name, "update"] as const,
    setup: (name: string) => [LOCAL_HOST, "hosts", "detail", name, "setup"] as const,
    lists: () => [LOCAL_HOST, "hosts", "list"] as const,
    list: () => [LOCAL_HOST, "hosts", "list", {}] as const,
    /** `GET /v1/host-setups`: every saved setup and update. */
    setups: () => [LOCAL_HOST, "host-setups", "list", {}] as const,
  },
  /** `GET /v1/remote-tasks/protocol`: the daemon identity behind one connection. */
  remoteTaskProtocol: (host: Host, endpoint: string) =>
    [host, "remote-task-protocol", endpoint] as const,
  pullRequests: {
    all: (host: Host) => [host, "pull-requests"] as const,
    lists: (host: Host) => [host, "pull-requests", "list"] as const,
    list: (
      host: Host,
      filters?: { repo?: string; role?: string; requested?: boolean; q?: string },
    ) => [host, "pull-requests", "list", filters ?? {}] as const,
    /** By Ariadne's id of a request it works on, or by `repository:number`. */
    detail: (host: Host, id: string) => [host, "pull-requests", "detail", id] as const,
    search: (host: Host, repo: string, q: string) =>
      [host, "pull-requests", "search", { repo, q }] as const,
  },
  issues: {
    list: (host: Host, repository: string, assigned: "me" | "all") =>
      [host, "issues", "list", repository, { assigned }] as const,
    /** Both assignment filters of one repository's issues. */
    ofRepository: (host: Host, repository: string) => [host, "issues", "list", repository] as const,
  },
  goals: {
    all: (host: Host) => [host, "goals"] as const,
    lists: (host: Host) => [host, "goals", "list"] as const,
    list: (host: Host, filters?: PageFilters) => [host, "goals", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "goals", "detail"] as const,
    detail: (host: Host, id: string) => [host, "goals", "detail", id] as const,
  },
  tasks: {
    all: (host: Host) => [host, "tasks"] as const,
    lists: (host: Host) => [host, "tasks", "list"] as const,
    list: (host: Host, filters?: TaskFilters) => [host, "tasks", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "tasks", "detail"] as const,
    detail: (host: Host, id: string) => [host, "tasks", "detail", id] as const,
    transitions: (host: Host, id: string) => [host, "tasks", "detail", id, "transitions"] as const,
    diff: (host: Host, id: string) => [host, "tasks", "detail", id, "diff"] as const,
  },
  sessions: {
    all: (host: Host) => [host, "sessions"] as const,
    lists: (host: Host) => [host, "sessions", "list"] as const,
    list: (host: Host, filters?: SessionFilters) =>
      [host, "sessions", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "sessions", "detail"] as const,
    detail: (host: Host, id: string) => [host, "sessions", "detail", id] as const,
    /** The session's pending agent requests (`GET /v1/sessions/{id}/agent-requests`). */
    agentRequests: (host: Host, id: string) =>
      [host, "sessions", "detail", id, "agent-requests"] as const,
    /** The session's open native questions (`GET /v1/sessions/{id}/questions`). */
    questions: (host: Host, id: string) => [host, "sessions", "detail", id, "questions"] as const,
  },
  skills: {
    all: (host: Host) => [host, "skills"] as const,
    lists: (host: Host) => [host, "skills", "list"] as const,
    list: (host: Host, filters?: PageFilters) => [host, "skills", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "skills", "detail"] as const,
    detail: (host: Host, name: string) => [host, "skills", "detail", name] as const,
  },
  workflows: {
    all: (host: Host) => [host, "workflows"] as const,
    lists: (host: Host) => [host, "workflows", "list"] as const,
    list: (host: Host) => [host, "workflows", "list", {}] as const,
    details: (host: Host) => [host, "workflows", "detail"] as const,
    detail: (host: Host, name: string) => [host, "workflows", "detail", name] as const,
    parse: (host: Host, document: string) => [host, "workflows", "parse", document] as const,
  },
  /**
   * The registered checkouts goals are created against
   * (`GET /v1/repositories`). One unfiltered list — the daemon takes no
   * filters — so `list()` is always keyed by the empty filter object.
   */
  repositories: {
    all: (host: Host) => [host, "repositories"] as const,
    lists: (host: Host) => [host, "repositories", "list"] as const,
    list: (host: Host, filters?: PageFilters) =>
      [host, "repositories", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "repositories", "detail"] as const,
    detail: (host: Host, id: string) => [host, "repositories", "detail", id] as const,
  },
  /**
   * How each registry agent is launched (`GET /v1/agents`): one unfiltered
   * list of every agent kind, because the daemon answers with all of them.
   */
  agents: {
    all: (host: Host) => [host, "agents"] as const,
    lists: (host: Host) => [host, "agents", "list"] as const,
    list: (host: Host) => [host, "agents", "list", {}] as const,
  },
  /**
   * The authoritative "Needs attention" list (`GET /v1/attention`), one
   * unfiltered list the daemon's own producers fill — `attention.ts` folds
   * it into the task/session rows it derives itself rather than reading it
   * blind.
   */
  attention: {
    all: (host: Host) => [host, "attention"] as const,
    lists: (host: Host) => [host, "attention", "list"] as const,
    list: (host: Host) => [host, "attention", "list", {}] as const,
  },
  /** The model catalog (`GET /v1/models`), one unfiltered list for all agents. */
  models: {
    all: (host: Host) => [host, "models"] as const,
    lists: (host: Host) => [host, "models", "list"] as const,
    list: (host: Host) => [host, "models", "list", {}] as const,
  },
  /**
   * The ACP agent registry and its cached discovery result
   * (`GET /v1/acp-agents`), one unfiltered list.
   */
  acpAgents: {
    all: (host: Host) => [host, "acp-agents"] as const,
    lists: (host: Host) => [host, "acp-agents", "list"] as const,
    list: (host: Host) => [host, "acp-agents", "list", {}] as const,
  },
  /** Raw agent events the ACP runtime recorded (`GET /v1/events`). */
  agentEvents: {
    all: (host: Host) => [host, "agent-events"] as const,
    lists: (host: Host) => [host, "agent-events", "list"] as const,
    list: (host: Host, filters?: AgentEventFilters) =>
      [host, "agent-events", "list", filters ?? {}] as const,
  },
  /**
   * The AI permission model's settings, behind the `ai` permission mode
   * (`GET /v1/permissions/ai`). One row, so a detail key and no list.
   */
  permissions: {
    ai: (host: Host) => [host, "permissions", "detail", "ai"] as const,
  },
  /**
   * The webhook tunnel's switch and state (`GET /v1/forge/tunnel`). One row,
   * so a detail key and no list.
   */
  forge: {
    tunnel: (host: Host) => [host, "forge", "detail", "tunnel"] as const,
  },
  learnedPermissions: {
    all: (host: Host) => [host, "learned-permissions"] as const,
    lists: (host: Host) => [host, "learned-permissions", "list"] as const,
    list: (host: Host, filters?: LearnedPermissionFilters) =>
      [host, "learned-permissions", "list", filters ?? {}] as const,
    details: (host: Host) => [host, "learned-permissions", "detail"] as const,
    detail: (host: Host, id: string) => [host, "learned-permissions", "detail", id] as const,
  },
  /**
   * Perses dashboard documents (`GET /v1/dashboards`), keyed by name rather
   * than id — `DashboardDto` carries no id of its own.
   */
  dashboards: {
    all: (host: Host) => [host, "dashboards"] as const,
    lists: (host: Host) => [host, "dashboards", "list"] as const,
    list: (host: Host) => [host, "dashboards", "list", {}] as const,
    details: (host: Host) => [host, "dashboards", "detail"] as const,
    detail: (host: Host, name: string) => [host, "dashboards", "detail", name] as const,
  },
} as const
