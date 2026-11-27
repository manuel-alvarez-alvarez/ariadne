/**
 * The status row under the composer, and the subtitle of the console.
 *
 * The row shows the session's status, the running turn and its clock, then
 * what the trajectory's session state says: the mode, each config option,
 * the context use and the cost, and at its end the token totals. It is one
 * line at every width: a fact that does not fit is cut, never wrapped.
 *
 * The subtitle is the title the agent gave the session, else the heading the
 * session panel shows.
 */

import type { SessionDto, SessionStatus, TrajectorySnapshotDto } from "@/api"
import { formatTokens } from "@/lib/format"

import { StatusRow } from "../console/status"
import type { TokenTotals } from "../console/transcript"
import { sessionHeading } from "../session-display"

type SessionState = TrajectorySnapshotDto["session"]

export function TrajectoryStatus({
  status,
  turn,
  waiting,
  tokens,
  session,
}: {
  status: SessionStatus
  turn?: { tool?: string; seconds?: number }
  waiting: boolean
  tokens: TokenTotals
  /** The session's state from the trajectory, once a snapshot said it. */
  session?: SessionState
}) {
  const facts = session ? stateFacts(session) : []
  return (
    <StatusRow
      status={status}
      turn={turn}
      waiting={waiting}
      tokens={tokens}
      facts={
        facts.length > 0 ? (
          <span className="flex min-w-0 items-center gap-3 overflow-hidden">
            {facts.map((fact) => (
              <span key={fact.key} title={fact.hint} className="min-w-0 truncate">
                {fact.text}
              </span>
            ))}
          </span>
        ) : null
      }
    />
  )
}

/**
 * The title the agent gave the session, under the console's stream line. With
 * none, the heading the session panel shows.
 */
export function ConsoleSubtitle({
  title,
  session,
}: {
  title?: string | null
  session?: Pick<SessionDto, "seat">
}) {
  title ||= session ? sessionHeading(session) : null
  if (!title) return null
  return (
    <p
      role="note"
      aria-label="Session title"
      title={title}
      className="min-w-0 truncate font-sans text-xs text-foreground"
    >
      {title}
    </p>
  )
}

interface Fact {
  key: string
  text: string
  hint?: string
}

/** The mode, the config options, the context use and the cost, in that order. */
function stateFacts(session: SessionState): Fact[] {
  const facts: Fact[] = []
  const mode = session.mode
  if (mode?.current_id) {
    const name = mode.available.find((one) => one.id === mode.current_id)?.name ?? mode.current_id
    facts.push({ key: "mode", text: `mode ${name}` })
  }
  for (const option of session.config_options) {
    if (!option.current_value) continue
    const value =
      option.values.find((one) => one.value === option.current_value)?.name ?? option.current_value
    facts.push({ key: `option:${option.id}`, text: `${option.name} ${value}` })
  }
  const context = session.context
  if (context && context.size > 0) {
    const share = Math.round((context.used / context.size) * 100)
    facts.push({
      key: "context",
      text: `context ${share}%`,
      hint: `${formatTokens(context.used)} of ${formatTokens(context.size)} tokens`,
    })
  }
  if (session.cost) facts.push({ key: "cost", text: money(session.cost) })
  return facts
}

/** An amount in its currency, as `$0.42`; a code the browser does not know reads as `0.42 XYZ`. */
function money({ amount, currency }: { amount: number; currency: string }): string {
  try {
    return new Intl.NumberFormat("en", { style: "currency", currency }).format(amount)
  } catch {
    return `${amount.toFixed(2)} ${currency}`
  }
}
