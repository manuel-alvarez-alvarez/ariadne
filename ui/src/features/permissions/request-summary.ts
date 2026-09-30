/**
 * A short line of what an agent actually asked to do, from a learned row's own
 * `tool_call`: the command of a shell call, the path of a file call, or the
 * call compacted to one line where it is neither.
 *
 * The ACP tool call the daemon stores keeps the agent's own arguments under
 * `rawInput` (`crates/ariadne-daemon/src/acp.rs`: `tool_call:
 * Some(params["toolCall"].clone())`, whose `command` and `file_path` live a
 * level under that, never on the call itself).
 *
 * Its own file rather than a fold into `learned-tab.tsx`: the delete dialog
 * needs it too, and a dialog reaching into the tab that opens it would be a
 * cycle for a one-function reason.
 */
export function requestSummary(toolCall: unknown): string | null {
  if (toolCall === null || toolCall === undefined) return null
  if (typeof toolCall === "object") {
    const rawInput = (toolCall as Record<string, unknown>).rawInput
    if (rawInput !== null && typeof rawInput === "object") {
      const input = rawInput as Record<string, unknown>
      if (typeof input.command === "string") return input.command
      if (typeof input.file_path === "string") return input.file_path
      if (typeof input.path === "string") return input.path
    }
  }
  try {
    return JSON.stringify(toolCall)
  } catch {
    return String(toolCall)
  }
}

/** Whether a row's selected option approved the request or refused it. */
type SelectedOptionOutcome = "allow" | "deny"

/**
 * What a row's `selected_option` means, read out of its own `options`: the
 * option's name, where the ACP `options` array carries one (the id
 * otherwise), and whether it is an approval or a refusal, from its `kind`
 * (`allow_once` / `allow_always` vs. `reject_once` / `reject_always` — see
 * `allowing_option` in `crates/ariadne-daemon/src/acp.rs`). `outcome` is
 * `null` where `options` does not carry an entry for `selected_option`,
 * which the contract does not promise cannot happen.
 */
export function selectedOptionInfo(
  options: unknown,
  selectedOption: string,
): { label: string; outcome: SelectedOptionOutcome | null } {
  const match = Array.isArray(options)
    ? (options as unknown[]).find(
        (option): option is Record<string, unknown> =>
          option !== null &&
          typeof option === "object" &&
          (option as Record<string, unknown>).optionId === selectedOption,
      )
    : undefined
  const name = match && typeof match.name === "string" ? match.name : undefined
  const kind = match && typeof match.kind === "string" ? match.kind : undefined
  return {
    label: name ?? selectedOption,
    outcome: kind ? (kind.startsWith("allow") ? "allow" : "deny") : null,
  }
}
