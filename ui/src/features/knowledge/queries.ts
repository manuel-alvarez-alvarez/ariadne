/**
 * The knowledge screen's reads and writes (022): a repository's status, a
 * reindex, the interactions of one ref, and the symbol reads the Impact and
 * path tab makes.
 *
 * Reindex answers 202 with no body — the rebuild runs in the background — so
 * the mutation flips the cached status to `indexing` itself rather than
 * waiting on a refetch; `knowledge_indexed` / `knowledge_failed` are what
 * bring the real state back (`@/events/dispatch.ts`).
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, type KnowledgeDetail, type KnowledgeStatusDto, qk, unwrap } from "@/api"

/** `GET /v1/repositories/{id}/knowledge`. */
export function knowledgeStatusQueryOptions(repositoryId: string) {
  return queryOptions({
    queryKey: qk.repositories.knowledgeStatus(repositoryId),
    queryFn: () =>
      unwrap(
        api().GET("/v1/repositories/{id}/knowledge", {
          params: { path: { id: repositoryId } },
        }),
      ),
  })
}

export function useReindexKnowledge(repositoryId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      unwrap(
        api().POST("/v1/repositories/{id}/knowledge/reindex", {
          params: { path: { id: repositoryId } },
        }),
      ),
    onSuccess: () => {
      queryClient.setQueryData(
        qk.repositories.knowledgeStatus(repositoryId),
        (status: KnowledgeStatusDto | undefined) =>
          status ? { ...status, state: "indexing" as const, error: null } : status,
      )
    },
  })
}

/** `GET /v1/knowledge/search`, scoped by the screen's repository and ref. */
export function knowledgeSearchQueryOptions(
  repositoryId: string,
  gitRef: string,
  filters: { q: string; kind?: string; path?: string },
) {
  const query = {
    repository: repositoryId,
    git_ref: gitRef,
    q: filters.q,
    ...(filters.kind ? { kind: filters.kind } : {}),
    ...(filters.path ? { path: filters.path } : {}),
  }
  return queryOptions({
    queryKey: qk.repositories.knowledgeSearch(repositoryId, query),
    queryFn: () => unwrap(api().GET("/v1/knowledge/search", { params: { query } })),
    enabled: filters.q.length > 0,
  })
}

/** One name's definitions, either with its neighbourhood or with its source. */
export function knowledgeSymbolQueryOptions(
  repositoryId: string,
  gitRef: string,
  name: string,
  detail: Extract<KnowledgeDetail, "context" | "source">,
) {
  const query = { repository: repositoryId, git_ref: gitRef, name, detail }
  return queryOptions({
    queryKey: qk.repositories.knowledgeSymbol(repositoryId, query),
    queryFn: () => unwrap(api().GET("/v1/knowledge/symbol", { params: { query } })),
    enabled: name.length > 0,
  })
}

/**
 * `GET /v1/knowledge/interactions` for one repository: the edges between it
 * and every other repository, both ways. Without a ref the daemon reads the
 * repository's own base.
 */
export function knowledgeInteractionsQueryOptions(repositoryId: string, gitRef?: string) {
  return queryOptions({
    queryKey: qk.repositories.knowledgeInteractions(repositoryId, {
      repository: repositoryId,
      git_ref: gitRef,
    }),
    queryFn: () =>
      unwrap(
        api().GET("/v1/knowledge/interactions", {
          params: { query: { repository: repositoryId, ...(gitRef ? { git_ref: gitRef } : {}) } },
        }),
      ),
  })
}

/** `GET /v1/knowledge/impact`: every definition of `symbol`, and the callers each one reaches. */
export function knowledgeImpactQueryOptions(
  repositoryId: string,
  gitRef: string,
  symbol: string,
  depth: number,
) {
  return queryOptions({
    queryKey: qk.repositories.knowledgeImpact(repositoryId, {
      repository: repositoryId,
      git_ref: gitRef,
      symbol,
      depth,
    }),
    queryFn: () =>
      unwrap(
        api().GET("/v1/knowledge/impact", {
          params: { query: { repository: repositoryId, git_ref: gitRef, symbol, depth } },
        }),
      ),
    enabled: symbol.length > 0,
  })
}

/** `GET /v1/knowledge/path`: the shortest directed path between two symbol names. */
export function knowledgePathQueryOptions(
  repositoryId: string,
  gitRef: string,
  from: string,
  to: string,
  depth: number,
) {
  return queryOptions({
    queryKey: qk.repositories.knowledgePath(repositoryId, {
      repository: repositoryId,
      git_ref: gitRef,
      from,
      to,
      depth,
    }),
    queryFn: () =>
      unwrap(
        api().GET("/v1/knowledge/path", {
          params: { query: { repository: repositoryId, git_ref: gitRef, from, to, depth } },
        }),
      ),
    enabled: from.length > 0 && to.length > 0,
  })
}
