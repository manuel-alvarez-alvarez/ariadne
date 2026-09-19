/**
 * The knowledge-base vocabulary (022): the values the daemon writes into the
 * string fields of an interaction edge.
 *
 * The DTOs themselves are aliases of the generated schema, in `@/api/types.ts`
 * with the rest of the app's. The schema types `kind`, `confidence` and
 * `step` as plain strings, so the values the screen knows how to name are
 * listed here, and a value outside them is shown as the daemon wrote it.
 */

export type KnowledgeInteractionKind = "depends_on" | "references" | "calls_route" | "sets_env"
export type KnowledgeConfidence = "exact" | "heuristic"

/** Which step joined an edge's two ends: what the confidence rests on. */
export type KnowledgeStep =
  | "file"
  | "directory"
  | "import"
  | "repository"
  | "path"
  | "route"
  | "name"
