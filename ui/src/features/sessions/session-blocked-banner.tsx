/**
 * What to do about an agent that is blocked, said where the agent is.
 *
 * The daemon's reason used to be a badge with a tooltip on it, next to a
 * terminal whose only instruction was "click to type" — which leaves the two
 * halves of the answer (that this agent is waiting on a keystroke, and which
 * keystroke) on opposite sides of a hover. A prompt is answered in the pane
 * and nowhere else, so the panel says so in a line nobody has to find, above
 * the pane it is talking about.
 *
 * `y` and Return is the answer to almost every permission prompt every one of
 * the three agent CLIs raises, so it is also a button: the whole interaction is
 * one click for the common case, and the sentence next to it is what to do for
 * the rest. It goes through the same `POST /v1/sessions/{id}/input` a keystroke
 * does — same ordering, same coalescing (see `queries.ts`) — because it *is* a
 * keystroke, and the pane echoing it back is what confirms it landed.
 */

import { TriangleAlertIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type { SessionDto } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/format"

import { sendSessionInput } from "./queries"
import { isLiveStatus } from "./session-display"

/** What answers a permission prompt: `y`, then Return. */
const APPROVE = "y\r"

export function SessionBlockedBanner({ session }: { session: SessionDto }) {
  const [sending, setSending] = useState(false)
  const reason = session.attention_reason
  // Only the two the pane itself is waiting on. `waiting_user` is something
  // else the user owes the session, not a keystroke in this pane.
  if (reason !== "waiting_permission" && reason !== "waiting_input") return null
  const permission = reason === "waiting_permission"
  // A pane that is gone cannot be typed into; the reason outlives it.
  const live = isLiveStatus(session.status)

  function approve() {
    setSending(true)
    sendSessionInput(session.id, APPROVE)
      .catch((error: unknown) => {
        toast.error("Could not answer the prompt", {
          id: `session-input-${session.id}`,
          description: describeError(error),
        })
      })
      .finally(() => setSending(false))
  }

  return (
    <Alert className="border-status-warn/40 bg-status-warn-soft/50">
      <TriangleAlertIcon className="text-status-warn-fg" />
      <AlertTitle>
        {permission ? "Blocked on a permission prompt" : "The agent asked a question"}
      </AlertTitle>
      <AlertDescription>
        {live ? (
          permission ? (
            <>
              Type into the terminal below — <Key>y</Key> then Enter usually approves.
            </>
          ) : (
            <>Type the answer into the terminal below, then Enter.</>
          )
        ) : (
          <>Its pane is gone, so there is nothing left to type into: resume the session first.</>
        )}
      </AlertDescription>
      {permission && live ? (
        <AlertAction>
          <Button size="xs" variant="outline" onClick={approve} pending={sending}>
            Approve
          </Button>
        </AlertAction>
      ) : null}
    </Alert>
  )
}

function Key({ children }: { children: string }) {
  return (
    <kbd className="rounded border bg-muted px-1 font-mono text-[0.7rem] leading-4">{children}</kbd>
  )
}
