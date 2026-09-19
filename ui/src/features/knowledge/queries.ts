/**
 * The knowledge screen's reads and writes (022): a repository's status, a
 * reindex, and the interactions of one ref.
 *
 * Reindex answers 202 with no body — the rebuild runs in the background — so
 * the mutation flips the cached status to `indexing` itself rather than
 * waiting on a refetch; `knowledge_indexed` / `knowledge_failed` are what
 * bring the real state back (`@/events/dispatch.ts`).
 */

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import { api, type KnowledgeStatusDto, qk, unwrap } from "@/api"

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
