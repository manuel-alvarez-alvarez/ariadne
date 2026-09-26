/**
 * One staffed agent in two lines: its skills, wrapped over as many lines as
 * they need, and below them the model it runs on, in muted text.
 *
 * An agent has no name and no page: it is what it knows and what it runs on.
 * So the skills are the identity — each one linking to itself — and the model
 * beneath them is quiet secondary text, drawn by {@link ModelPin} in its
 * wrapping mode so a long id breaks rather than clips in a fact cell half a
 * panel wide.
 *
 * There is nothing behind the pin to disagree with it. What the orchestrator
 * sized this agent at, or what the user chose instead, is simply what it runs
 * on — which is why this says one thing where the profile summary it replaces
 * had to say which of two won.
 */

import type { Seat } from "@/api"
import { SkillName } from "@/features/skills/skill-name"
import { cn, SEAT_LABELS } from "@/lib/format"

import { ModelPin } from "./model-pin"

export function AgentSummary({
  skills,
  model,
  effort,
  className,
}: {
  /** The skills this agent loads, in the order they reach it. */
  skills: string[]
  /**
   * What it runs on, as the one qualified id that says it.
   */
  model: string
  /**
   * The effort that model is run at: null is the agent's own, which shows
   * as nothing at all — an effort nobody pinned is not a fact about this
   * agent.
   */
  effort?: string | null
  className?: string
}) {
  return (
    <span className={cn("flex min-w-0 flex-col gap-0.5", className)}>
      <span className="flex flex-wrap items-baseline gap-x-1">
        {skills.length === 0 ? (
          // Legal, and rarely what anybody wanted: an agent with nothing but
          // its task. Said plainly rather than left as a gap.
          <span className="text-muted-foreground italic">no skills</span>
        ) : (
          skills.map((skill, at) => (
            // No box to clip the name: the line wraps whole skills onto as
            // many lines as it needs, rather than cutting one short. A flex
            // item's default min-width is its content's, which would push a
            // long unbroken name past the cell instead of wrapping it — so
            // this one is let shrink (`min-w-0`) and break inside itself
            // (`break-words`) where a name has nowhere else to fold.
            <span key={skill} className="min-w-0 break-words">
              {at > 0 ? <span className="text-muted-foreground">, </span> : null}
              <SkillName name={skill} />
            </span>
          ))
        )}
      </span>
      <ModelPin model={model} effort={effort} mode="wrap" className="text-muted-foreground" />
    </span>
  )
}

/**
 * A session's agent in two lines: the seat it sits in, and below it what it
 * was launched on.
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
  model: string
  effort?: string | null
  className?: string
}) {
  return (
    <span className={cn("flex min-w-0 flex-col gap-0.5", className)}>
      <span>{SEAT_LABELS[seat]}</span>
      <ModelPin model={model} effort={effort} mode="wrap" className="text-muted-foreground" />
    </span>
  )
}
