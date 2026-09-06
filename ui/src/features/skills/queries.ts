import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query"

import {
  api,
  type CreateSkillRequest,
  cacheRow,
  dropRow,
  qk,
  type UpdateSkillRequest,
  unwrap,
} from "@/api"

/**
 * Every skill, shipped and written. One unfiltered list — the daemon takes no
 * filters — so it is keyed once and shared by the skills screen, the task form
 * and every mention of a skill.
 */
export function skillsQueryOptions() {
  return queryOptions({
    queryKey: qk.skills.list(),
    queryFn: () => unwrap(api().GET("/v1/skills")),
  })
}

export function useCreateSkill() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: CreateSkillRequest) => unwrap(api().POST("/v1/skills", { body })),
    onSuccess: (skill) => cacheRow(queryClient, qk.skills, skill),
  })
}

export function useUpdateSkill() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ name, body }: { name: string; body: UpdateSkillRequest }) =>
      unwrap(api().PUT("/v1/skills/{name}", { params: { path: { name } }, body })),
    onSuccess: (skill) => cacheRow(queryClient, qk.skills, skill),
  })
}

/**
 * Deletes a skill of the user's own. A shipped one is refused by the daemon —
 * it is reset rather than removed — so the screen only offers this where
 * `builtin` is false.
 */
export function useDeleteSkill() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api().DELETE("/v1/skills/{name}", { params: { path: { name } } })),
    onSuccess: (_result, name) => dropRow(queryClient, qk.skills, name),
  })
}

/**
 * Puts a shipped skill back on the document Ariadne ships, by dropping what
 * was written over it. Refused for a skill of the user's own, which has no
 * shipped text behind it.
 */
export function useResetSkill() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) =>
      unwrap(api().POST("/v1/skills/{name}/document/reset", { params: { path: { name } } })),
    onSuccess: (skill) => cacheRow(queryClient, qk.skills, skill),
  })
}
