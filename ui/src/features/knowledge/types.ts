/**
 * The knowledge-base DTOs (022): what one repository's index looks like, a
 * search hit, and an interaction edge.
 *
 * The daemon side of 022 lands separately, so none of these are in the
 * generated schema yet — hand-written here rather than aliased out of
 * `@/api/types.ts` like the rest of the app's DTOs. Move these into that file
 * (as aliases of the generated schema) once `npm run gen:api` picks the real
 * endpoints up.
 */

/** `idle` while nothing is wrong and nothing is running, `disabled` while the feature is off. */
export type KnowledgeState = "idle" | "indexing" | "failed" | "disabled"

interface KnowledgeRefDto {
  git_ref: string
  commit: string
  indexed_at: string
}

interface KnowledgeLanguageDto {
  language: string
  files: number
}

/** `GET /v1/repositories/{id}/knowledge`. */
export interface KnowledgeStatusDto {
  repository_id: string
  state: KnowledgeState
  refs: KnowledgeRefDto[]
  files: number
  symbols: number
  languages: KnowledgeLanguageDto[]
  error?: string | null
}

/** One row of `GET /v1/knowledge/search`. */
export interface KnowledgeSearchResultDto {
  repository_id: string
  path: string
  line: number
  kind: string
  name: string
  signature?: string | null
}

export type KnowledgeInteractionKind = "depends_on" | "references" | "calls_route" | "sets_env"
export type KnowledgeConfidence = "exact" | "heuristic"

export interface KnowledgeEndpointDto {
  repository_id: string
  path: string
  line: number
  symbol: string
}

/** Which step joined an edge's two ends: what the confidence rests on. */
export type KnowledgeStep =
  | "file"
  | "directory"
  | "import"
  | "repository"
  | "path"
  | "route"
  | "name"

interface KnowledgeEdgeDto {
  from: KnowledgeEndpointDto
  to: KnowledgeEndpointDto
  confidence: KnowledgeConfidence
  step: KnowledgeStep
  /** How many definitions matched at that step. */
  candidates: number
}

/** One group of `GET /v1/knowledge/interactions`, grouped by kind on the wire. */
export interface KnowledgeInteractionGroupDto {
  kind: KnowledgeInteractionKind
  edges: KnowledgeEdgeDto[]
}
