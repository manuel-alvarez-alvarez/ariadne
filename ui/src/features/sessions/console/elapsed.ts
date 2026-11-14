/** The time from one stored instant to a later one, as the CLI's clock reads it. */
export function elapsed(from: string, to: string): string | undefined {
  const ms = Date.parse(to) - Date.parse(from)
  if (Number.isNaN(ms) || ms < 0) return undefined
  const seconds = Math.floor(ms / 1000)
  if (seconds < 60) return `${seconds}s`
  return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`
}
