/**
 * The shapes the turn console shares with its composer, its permission card
 * and its status row.
 */

export interface PermissionOption {
  id: string
  name: string
  /** `allow_once`, `allow_always`, `reject_once` or `reject_always`. */
  kind?: string
}

/** A slash command the agent lists: an ACP `AvailableCommand`. */
export interface AgentCommand {
  /** The name, without its `/`. */
  name: string
  description: string
  /** What its argument is, as the agent says it. */
  hint?: string
}

export interface TokenTotals {
  input: number
  output: number
}
