/**
 * The status row under the composer, and the subtitle of the console.
 *
 * The row shows the session's status as the trajectory says it, the running
 * turn and its clock, the token totals, then what the session state says: the
 * context use, the model, the mode, each other config option and the cost. It
 * is one line at every width: a fact that does not fit is dropped whole, the
 * lowest rank first, never cut.
 *
 * The subtitle is the title the agent gave the session, where it gave one.
 */

import type { SessionStatus, TrajectorySnapshotDto } from "@/api"
import { formatTokens } from "@/lib/format"

import { type StatusFact, StatusRow } from "../console/status"
import type { TokenTotals } from "../console/transcript"

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
  return (
    <StatusRow
      status={session?.status ?? status}
      turn={turn}
      waiting={waiting}
      tokens={tokens}
      facts={session ? stateFacts(session) : []}
    />
  )
}

/** The title the agent gave the session, under the console's stream line. */
export function ConsoleSubtitle({ title }: { title?: string | null }) {
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

/**
 * The context use, the model, the mode, each other option, then the cost: the
 * rank of the facts after the tokens, highest first. The mode shows once,
 * from the mode state, else from the option of its category.
 */
function stateFacts(session: SessionState): StatusFact[] {
  const options = session.config_options.filter((option) => option.current_value)
  const currentName = (option: (typeof options)[number]) =>
    option.values.find((one) => one.value === option.current_value)?.name ?? option.current_value
  const facts: StatusFact[] = []
  const context = session.context
  if (context && context.size > 0) {
    const share = Math.round((context.used / context.size) * 100)
    facts.push({
      key: "context",
      text: `context ${share}%`,
      hint: `${formatTokens(context.used)} of ${formatTokens(context.size)} tokens`,
    })
  }
  const optionFact = (option: (typeof options)[number]): StatusFact => ({
    key: `option:${option.id}`,
    text: `${option.name} ${currentName(option)}`,
  })
  for (const option of options.filter((one) => one.category === "model")) {
    facts.push(optionFact(option))
  }
  const mode = session.mode
  const modeOption = options.find((one) => one.category === "mode")
  if (mode?.current_id) {
    const name = mode.available.find((one) => one.id === mode.current_id)?.name ?? mode.current_id
    facts.push({ key: "mode", text: `mode ${name}` })
  } else if (modeOption) {
    facts.push({ key: "mode", text: `mode ${currentName(modeOption)}` })
  }
  for (const option of options) {
    if (option.category !== "model" && option.category !== "mode") facts.push(optionFact(option))
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
