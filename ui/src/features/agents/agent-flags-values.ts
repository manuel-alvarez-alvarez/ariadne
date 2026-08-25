/**
 * The flags form's own value shape, and the two questions the screen asks
 * about a flag list.
 *
 * Flags are held as objects rather than as bare strings because
 * `useFieldArray` keys rows by identity: an array of strings loses the row a
 * value belongs to as soon as one is removed. They come back out as an argv
 * line — `PUT /v1/agents/{kind}` replaces the list whole — so blank rows are
 * dropped and the edges trimmed on the way out, which is also why a row may be
 * left empty while it is being typed into and never needs validating.
 */

import { z } from "zod"

export const agentFlagsSchema = z.object({
  flags: z.array(z.object({ value: z.string() })),
})

export type AgentFlagsFormValues = z.infer<typeof agentFlagsSchema>

type FlagRow = AgentFlagsFormValues["flags"][number]

/** A stored flag list as form rows. */
export function flagRows(flags: readonly string[]): FlagRow[] {
  return flags.map((value) => ({ value }))
}

/** The rows as the daemon takes them: trimmed, with the blank ones dropped. */
export function cleanFlags(rows: readonly FlagRow[]): string[] {
  return rows.map((row) => row.value.trim()).filter((flag) => flag.length > 0)
}

/**
 * Whether two flag lists are the same argv, order included.
 *
 * Order matters to a CLI, so this is not a set comparison: `--a --b` and
 * `--b --a` are two different launches and count as customized.
 */
export function sameFlags(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((flag, index) => flag === right[index])
}
