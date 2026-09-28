/**
 * A short line of what an agent actually asked to do, from a learned row's own
 * `tool_call`: the command of a shell call, the path of a file call, or the
 * call compacted to one line where it is neither. `null` for a manual row,
 * which recorded no request at all.
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
