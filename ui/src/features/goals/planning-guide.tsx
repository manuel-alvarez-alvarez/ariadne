/**
 * The next planning action of a goal, above its tabs: what its orchestrator
 * session has recorded, and the way into that conversation.
 *
 * Only what the daemon recorded is said. A session's own status and attention
 * reason decide the wording; no session, a failed read or a read still in
 * flight never become progress.
 */

import { useQuery } from "@tanstack/react-query"
import { MessagesSquareIcon, PlusIcon } from "lucide-react"

import type { GoalDto, SessionDto } from "@/api"
import { Button } from "@/components/ui/button"
import { When } from "@/components/when"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { sessionAttention } from "@/features/sessions/session-display"

/** What the recorded state of the planning conversation supports saying. */
type PlanningState =
  | { kind: "loading" }
  | { kind: "unavailable" }
  | { kind: "none" }
  | { kind: "starting" | "working" | "waiting" | "idle" | "ended"; session: SessionDto }

/** The newest orchestrator session decides; the list is read as the daemon sent it. */
function planningState(sessions: SessionDto[] | undefined, failed: boolean): PlanningState {
  // A failed read outranks cached data: the old state is not current.
  if (failed) return { kind: "unavailable" }
  if (!sessions) return { kind: "loading" }
  const session = sessions.reduce<SessionDto | null>(
    (latest, next) => (latest && latest.created_at >= next.created_at ? latest : next),
    null,
  )
  if (!session) return { kind: "none" }
  // A recorded wait comes before the status: a running agent can be blocked.
  const reason = sessionAttention(session)
  if (reason === "waiting_input" || reason === "waiting_permission") {
    return { kind: "waiting", session }
  }
  switch (session.status) {
    case "starting":
      return { kind: "starting", session }
    case "running":
      return { kind: "working", session }
    case "idle":
      return { kind: "idle", session }
    default:
      return { kind: "ended", session }
  }
}

const HEADLINES: Record<PlanningState["kind"], string> = {
  loading: "Checking the planning conversation",
  unavailable: "Planning state is unavailable",
  none: "No planning conversation recorded yet",
  starting: "The orchestrator is starting",
  working: "The orchestrator is working on the plan",
  waiting: "The orchestrator is waiting for you",
  idle: "The orchestrator is idle",
  ended: "The planning conversation has ended",
}

export function PlanningGuide({
  goal,
  onOpenConversation,
  onNewTask,
}: {
  goal: GoalDto
  /** Opens the orchestrator's session over the panel. */
  onOpenConversation: (sessionId: string) => void
  onNewTask: () => void
}) {
  // The key the Sessions tab and its count read, so this costs no request.
  const sessions = useQuery(sessionsQueryOptions({ goal: goal.id, seat: "orchestrator" }))
  const state = planningState(sessions.data, sessions.isError)
  const session = "session" in state ? state.session : null

  return (
    <section
      aria-label="Planning"
      className="flex flex-wrap items-center gap-2 rounded-lg border bg-card p-3"
    >
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{HEADLINES[state.kind]}</p>
        {"session" in state && state.session.last_activity_at ? (
          <p className="text-xs text-muted-foreground">
            last activity <When at={state.session.last_activity_at} label="last activity" />
          </p>
        ) : null}
      </div>
      {session ? (
        <Button size="sm" onClick={() => onOpenConversation(session.id)}>
          <MessagesSquareIcon />
          Open orchestrator conversation
        </Button>
      ) : null}
      <Button variant="outline" size="sm" onClick={onNewTask}>
        <PlusIcon />
        Create task manually
      </Button>
    </section>
  )
}
