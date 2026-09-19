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

import type { MemoryScope, SessionStatus, TaskStatus } from "./types"

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

/** What `GET /v1/outside-sessions` narrows its snapshot by, page aside. */
interface OutsideSessionFilters {
  agent?: string
  dir?: string
  since?: string
  until?: string
  q?: string
}

interface MemoryFilters {
  /** One repository's memories; omitted, every repository's. */
  repository?: string
  /** `repository`, `global` or `all`; the daemon defaults to `all`. */
  scope?: MemoryScope
  /** A substring search hits the daemon's search endpoint instead of list. */
  q?: string
}

/** `GET /v1/knowledge/search`, always narrowed to one repository from its page (022). */
interface KnowledgeSearchFilters {
  repository: string
  q?: string
  git_ref?: string
  kind?: string
  path?: string
}

/** `GET /v1/knowledge/interactions`, narrowed to one repository from its page (022). */
interface KnowledgeInteractionFilters {
  repository: string
  git_ref?: string
}

/** One name from `GET /v1/knowledge/symbol`, at one level of detail (022). */
interface KnowledgeSymbolFilters {
  repository: string
  git_ref: string
  name: string
  detail: "context" | "source"
}

/** `GET /v1/knowledge/impact`, for one changed symbol of one repository (022). */
interface KnowledgeImpactFilters {
  repository: string
  git_ref: string
  symbol: string
  depth: number
}

/** `GET /v1/knowledge/path`, between two symbols of one repository (022). */
interface KnowledgePathFilters {
  repository: string
  git_ref: string
  from: string
  to: string
  depth: number
}

/** `GET /v1/knowledge/graph`, for one repository from its page (022). */
interface KnowledgeGraphFilters {
  repository: string
  git_ref?: string
  limit?: number
}

/** `GET /v1/knowledge/outline`: one file of one repository (022). */
interface KnowledgeOutlineFilters {
  repository: string
  path: string
  git_ref?: string
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
    /** One repository's knowledge-base status (022): state, refs, counts, languages. */
    knowledgeStatus: (id: string) => ["repositories", "detail", id, "knowledge"] as const,
    /** Search over one repository's knowledge base, `q`/`kind`/`path` included. */
    knowledgeSearch: (id: string, filters?: KnowledgeSearchFilters) =>
      ["repositories", "detail", id, "knowledge-search", filters ?? { repository: id }] as const,
    knowledgeSymbol: (id: string, filters: KnowledgeSymbolFilters) =>
      ["repositories", "detail", id, "knowledge-symbol", filters] as const,
    /**
     * Every filtered interactions list for one repository. Shorter than
     * {@link knowledgeInteractions}'s own key on purpose — invalidating this
     * prefix catches a list under any `git_ref` without knowing which one is
     * open, the way `tasks.lists()` catches every filter of that list.
     */
    knowledgeInteractionsAll: (id: string) =>
      ["repositories", "detail", id, "knowledge-interactions"] as const,
    /**
     * Every impact and path walk for one repository, under any ref and
     * symbols: what an indexing run makes stale.
     */
    knowledgeWalksAll: (id: string) => ["repositories", "detail", id, "knowledge-walk"] as const,
    knowledgeImpact: (id: string, filters: KnowledgeImpactFilters) =>
      ["repositories", "detail", id, "knowledge-walk", "impact", filters] as const,
    knowledgePath: (id: string, filters: KnowledgePathFilters) =>
      ["repositories", "detail", id, "knowledge-walk", "path", filters] as const,
    knowledgeInteractions: (id: string, filters?: KnowledgeInteractionFilters) =>
      [
        "repositories",
        "detail",
        id,
        "knowledge-interactions",
        filters ?? { repository: id },
      ] as const,
    /** Every file graph of one repository, whatever its ref and limit: the prefix events invalidate. */
    knowledgeGraphAll: (id: string) => ["repositories", "detail", id, "knowledge-graph"] as const,
    knowledgeGraph: (id: string, filters: KnowledgeGraphFilters) =>
      ["repositories", "detail", id, "knowledge-graph", filters] as const,
    /** Every file outline of one repository: the prefix events invalidate. */
    knowledgeOutlineAll: (id: string) =>
      ["repositories", "detail", id, "knowledge-outline"] as const,
    knowledgeOutline: (id: string, filters: KnowledgeOutlineFilters) =>
      ["repositories", "detail", id, "knowledge-outline", filters] as const,
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
   * Saved facts, of one repository or of none (`GET /v1/memories[/search]`),
   * narrowed by repository and scope.
   */
  memories: {
    all: () => ["memories"] as const,
    lists: () => ["memories", "list"] as const,
    list: (filters: MemoryFilters) => ["memories", "list", filters] as const,
  },
} as const
