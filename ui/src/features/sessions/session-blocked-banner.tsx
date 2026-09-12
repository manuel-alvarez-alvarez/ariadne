/**
 * What to do about an agent that is blocked, said where the agent is.
 *
 * The daemon's reason used to be a badge with a tooltip on it, which leaves
 * the two halves of the answer (that this agent is waiting on the user, and
 * where to answer it) on opposite sides of a hover. A permission question is
 * answered in the console and nowhere else, so the panel says so in a line
 * nobody has to find, above the console it is talking about.
 *
 * The console holds the answer itself: a permission question comes with the
 * options the agent offered, drawn as a picker in the terminal pane (see
 * `session-terminal.tsx`), so this line only points at it.
 */

import { TriangleAlertIcon } from "lucide-react"

import type { SessionDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"

import { isLiveStatus } from "./session-display"

export function SessionBlockedBanner({ session }: { session: SessionDto }) {
  const reason = session.attention_reason
  // Only the two the console itself is waiting on. `waiting_user` is
  // something else the user owes the session, not an answer in this console.
  if (reason !== "waiting_permission" && reason !== "waiting_input") return null
  const permission = reason === "waiting_permission"
  // An agent that is gone cannot be answered; the reason outlives it.
  const live = isLiveStatus(session.status)

  return (
    <Alert className="border-status-warn/40 bg-status-warn-soft/50">
      <TriangleAlertIcon className="text-status-warn-fg" />
      <AlertTitle>
        {permission ? "Blocked on a permission prompt" : "The agent asked a question"}
      </AlertTitle>
      <AlertDescription>
        {live ? (
          permission ? (
            <>Pick one of the options in the console below.</>
          ) : (
            <>Type the answer into the console below.</>
          )
        ) : (
          <>The agent is gone, so there is nothing left to answer: resume the session first.</>
        )}
      </AlertDescription>
    </Alert>
  )
}
