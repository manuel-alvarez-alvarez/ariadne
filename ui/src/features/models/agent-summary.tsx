/**
 * One staffed agent in a line: `skills · model`, and after an `@` the effort
 * that model is run at where one is pinned.
 *
 * An agent has no name and no page: it is what it knows and what it runs on.
 * So the skills are the identity — each one linking to itself — and the model
 * after them is quiet secondary text. Since a model names the agent CLI that
 * runs it (`claude_code:claude-opus-5`), that is one fact and not two.
 *
 * There is nothing behind the pin to disagree with it. What the orchestrator
 * sized this agent at, or what the user chose instead, is simply what it runs
 * on — which is why this says one thing where the profile summary it replaces
 * had to say which of two won.
 */

import type { Seat } from "@/api"
import { SkillName } from "@/features/skills/skill-name"
import { cn, SEAT_LABELS } from "@/lib/format"

import { pinLabel } from "./model-ref"

export function AgentSummary({
  skills,
  model,
  effort,
  className,
}: {
  /** The skills this agent loads, in the order they reach it. */
  skills: string[]
  /**
   * What it runs on, as the one qualified id that says it. Null is `auto` —
   * the first installed CLI, resolved at spawn time, on its own default model.
   */
  model?: string | null
  /**
   * The effort that model is run at: null is the agent CLI's own, which shows
   * as nothing at all — an effort nobody pinned is not a fact about this
   * agent.
   */
  effort?: string | null
  className?: string
}) {
  return (
    <span className={cn("flex min-w-0 items-baseline gap-1", className)}>
      <span className="flex min-w-0 shrink items-baseline gap-1">
        {skills.length === 0 ? (
          // Legal, and rarely what anybody wanted: an agent with nothing but
          // its task. Said plainly rather than left as a gap.
          <span className="truncate text-muted-foreground italic">no skills</span>
        ) : (
          skills.map((skill, at) => (
            <span key={skill} className="min-w-0 truncate">
              {at > 0 ? <span className="text-muted-foreground">, </span> : null}
              <SkillName name={skill} />
            </span>
          ))
        )}
      </span>
      <span className="min-w-0 truncate text-muted-foreground">· {pinLabel(model, effort)}</span>
    </span>
  )
}

/**
 * A session's agent in one line: the seat it sits in, and what it was launched
 * on.
 *
 * A session carries the seat and the pin but not the skills — those belong to
 * the staffed agent, which a session only points at — so this says what it
 * knows rather than fetching a task per row to say more.
 */
export function SeatSummary({
  seat,
  model,
  effort,
  className,
}: {
  seat: Seat
  model?: string | null
  effort?: string | null
  className?: string
}) {
  return (
    <span className={cn("flex min-w-0 items-baseline gap-1", className)}>
      <span className="shrink-0">{SEAT_LABELS[seat]}</span>
      <span className="min-w-0 truncate text-muted-foreground">· {pinLabel(model, effort)}</span>
    </span>
  )
}
