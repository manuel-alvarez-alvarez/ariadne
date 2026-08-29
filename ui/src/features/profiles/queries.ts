/**
 * The profile screen's reads and writes.
 *
 * Reads follow the app-wide key convention so the SSE dispatcher keeps them
 * live: `profile_created` / `profile_updated` / `profile_deleted` patch
 * `profiles.detail` and invalidate `profiles.lists`, which is every list here
 * whatever role it is filtered by — and which is what `cacheRow` / `dropRow`
 * do for the writes below.
 */

import { type QueryClient, queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"
import {
  api,
  type CreateProfileRequest,
  cacheRow,
  dropRow,
  type PageFilters,
  type ProfilePromptDto,
  type PromptKind,
  qk,
  type Role,
  type UpdateProfileRequest,
  unwrap,
} from "@/api"

/**
 * `GET /v1/profiles`, narrowed to one role by the daemon when given.
 *
 * The role goes into the key as a filter like any other: TanStack hashes the
 * filter object structurally, so each role gets its own cache entry and all of
 * them sit under `qk.profiles.lists()`.
 */
export function profilesQueryOptions(role?: Role) {
  const filters: PageFilters & { role?: Role } = role ? { role } : {}
  return queryOptions({
    queryKey: qk.profiles.list(filters),
    queryFn: () => unwrap(api().GET("/v1/profiles", { params: { query: { role } } })),
  })
}

/**
 * `GET /v1/models` — the model catalog, always the whole union.
 *
 * Unfiltered on purpose: the profile form re-scopes the list client-side as the
 * agent select changes, and the details panel looks a stored model up by its
 * agent kind, so one cache entry serves every consumer. The catalog is curated
 * (plus a live `opencode models` probe) and changes about as often as a CLI is
 * installed, so it is read once and then left alone, like every other read: the
 * client refetches when something invalidates it, never on a clock.
 */
export function modelsQueryOptions() {
  return queryOptions({
    queryKey: qk.models.list(),
    queryFn: () => unwrap(api().GET("/v1/models")),
  })
}

export function useCreateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateProfileRequest) => unwrap(api().POST("/v1/profiles", { body })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}

export function useUpdateProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateProfileRequest }) =>
      unwrap(api().PUT("/v1/profiles/{id}", { params: { path: { id } }, body })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}

export function useDeleteProfile() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().DELETE("/v1/profiles/{id}", { params: { path: { id } } })),
    onSuccess: (_result, id) => dropRow(queryClient, qk.profiles, id),
  })
}

/**
 * `GET /v1/profiles/{id}/prompts` — the briefing prompts of the profile's role,
 * in the order the daemon sends them, each one as it takes effect: the text set
 * on the profile, or the default of its kind with `is_default` saying so.
 *
 * Nested under the profile's detail key, so deleting a profile drops its
 * prompts with it. It is deliberately *not* refetched by `profile_updated`:
 * that event carries a `ProfileDto`, which the prompts are not part of, and a
 * refetch while someone is typing into an editor is exactly what should not
 * happen.
 */
export function profilePromptsQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.profiles.prompts(id),
    queryFn: () => unwrap(api().GET("/v1/profiles/{id}/prompts", { params: { path: { id } } })),
  })
}

/**
 * `PUT /v1/profiles/{id}/prompts/{kind}` — set the text of one prompt, which is
 * what makes it the profile's own.
 *
 * The profile is an argument rather than a binding of the hook because the
 * create dialog writes its prompts to a profile that did not exist when the
 * dialog rendered.
 */
export function useUpdateProfilePrompt() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, kind, content }: { id: string; kind: PromptKind; content: string }) =>
      unwrap(
        api().PUT("/v1/profiles/{id}/prompts/{kind}", {
          params: { path: { id, kind } },
          body: { content },
        }),
      ),
    onSuccess: (prompt, { id }) => cachePrompt(queryClient, id, prompt),
  })
}

/**
 * `POST /v1/profiles/{id}/prompts/{kind}/reset` — drop the text set on the
 * profile, leaving it on the default of the kind.
 *
 * Unlike an edit, this is written the moment it is asked for: the default is
 * the daemon's text and no form can hold a draft of it, so the answer is both
 * the write and the only way to read what it put back.
 */
export function useResetProfilePrompt() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, kind }: { id: string; kind: PromptKind }) =>
      unwrap(
        api().POST("/v1/profiles/{id}/prompts/{kind}/reset", { params: { path: { id, kind } } }),
      ),
    onSuccess: (prompt, { id }) => cachePrompt(queryClient, id, prompt),
  })
}

/** `POST /v1/profiles/{id}/system-prompt/reset` — the same, for the one prompt
 * that lives on the profile row rather than in the prompts list. */
export function useResetSystemPrompt() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/profiles/{id}/system-prompt/reset", { params: { path: { id } } })),
    onSuccess: (profile) => cacheRow(queryClient, qk.profiles, profile),
  })
}

/**
 * One prompt back into the list it came from, in place.
 *
 * A write answers with that prompt alone, so the list is patched rather than
 * invalidated: refetching would swap the array under editors that may be
 * holding unsaved drafts of the *other* prompts.
 */
function cachePrompt(queryClient: QueryClient, id: string, prompt: ProfilePromptDto): void {
  queryClient.setQueryData(qk.profiles.prompts(id), (prompts?: ProfilePromptDto[]) =>
    prompts?.map((current) => (current.kind === prompt.kind ? prompt : current)),
  )
}
