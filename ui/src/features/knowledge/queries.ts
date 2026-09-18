/**
 * The knowledge page's reads and writes (022): one repository's status, a
 * reindex, a search and its interactions.
 *
 * Reindex answers 202 with no body — the rebuild runs in the background — so
 * the mutation flips the cached status to `indexing` itself rather than
 * waiting on a refetch; `knowledge_indexed` / `knowledge_failed` are what
 * bring the real state back (`@/events/dispatch.ts`).
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { qk, unwrap } from "@/api"

import { knowledgeApi } from "./api"
import type { KnowledgeStatusDto } from "./types"

/** `GET /v1/repositories/{id}/knowledge`. */
export function knowledgeStatusQueryOptions(repositoryId: string) {
  return queryOptions({
    queryKey: qk.repositories.knowledgeStatus(repositoryId),
    queryFn: () =>
      unwrap(
        knowledgeApi().GET("/v1/repositories/{repository_id}/knowledge", {
          params: { path: { repository_id: repositoryId } },
        }),
      ),
  })
}

export function useReindexKnowledge(repositoryId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () =>
      unwrap(
        knowledgeApi().POST("/v1/repositories/{repository_id}/knowledge/reindex", {
          params: { path: { repository_id: repositoryId } },
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

/** `GET /v1/knowledge/search`, narrowed to one repository from its page. */
export function knowledgeSearchQueryOptions(
  repositoryId: string,
  filters: { q: string; kind: string; path: string },
) {
  const q = filters.q.trim()
  const kind = filters.kind.trim()
  const path = filters.path.trim()
  return queryOptions({
    queryKey: qk.repositories.knowledgeSearch(repositoryId, {
      repository: repositoryId,
      q: q || undefined,
      kind: kind || undefined,
      path: path || undefined,
    }),
    queryFn: () =>
      unwrap(
        knowledgeApi().GET("/v1/knowledge/search", {
          params: {
            query: {
              repository: repositoryId,
              ...(q ? { q } : {}),
              ...(kind ? { kind } : {}),
              ...(path ? { path } : {}),
            },
          },
        }),
      ),
    // Only once there is something to search for; an unfiltered search of the
    // whole repository is not what an empty box is asking for.
    enabled: q.length > 0 || kind.length > 0 || path.length > 0,
  })
}

/** `GET /v1/knowledge/interactions`, narrowed to one repository from its page. */
export function knowledgeInteractionsQueryOptions(repositoryId: string) {
  return queryOptions({
    queryKey: qk.repositories.knowledgeInteractions(repositoryId, { repository: repositoryId }),
    queryFn: () =>
      unwrap(
        knowledgeApi().GET("/v1/knowledge/interactions", {
          params: { query: { repository: repositoryId } },
        }),
      ),
  })
}
