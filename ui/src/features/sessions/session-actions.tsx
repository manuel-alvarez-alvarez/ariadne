/**
 * Kill and resume, the two things the UI can do *to* a session.
 *
 * Both are named with what they act on, like the goal's and the task's: these
 * sit in the same corner of a panel that is itself dismissible, and a bare
 * verb there reads as being about the panel.
 *
 * Kill is destructive and irreversible — it tears down the agent's tmux
 * process mid-thought — so it asks first, and because it asks, its refusal is
 * shown in that dialog like every other confirmed action's. Resume is not
 * destructive and has no dialog to put anything in; it also fails often and
 * for reasons worth reading (the daemon answers `409` when the session has no
 * agent-internal id to resume from, or is still running), so it toasts. See
 * `components/ui/sonner.tsx` for the rule those two are an instance of.
 */

import { PlayIcon, SkullIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type { SessionDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/errors"

import { useKillSession, useResumeSession } from "./queries"
import { isLiveStatus } from "./session-display"

export function SessionActions({
  session,
  onResumed,
}: {
  session: SessionDto
  /**
   * Handed the revived session — this same one, relaunched. Where to go from
   * there is the caller's call — a panel selects it — so these buttons stay
   * usable wherever they are rendered.
   */
  onResumed?: (session: SessionDto) => void
}) {
  const [confirmKill, setConfirmKill] = useState(false)
  const kill = useKillSession()
  const resume = useResumeSession()
  const live = isLiveStatus(session.status)

  return (
    <div className="flex items-center gap-2">
      {live ? (
        // Only opens the confirm; the solid red — and the spinner, since the
        // dialog is where the kill is actually running — are on the click
        // inside it.
        <Button variant="destructive-ghost" size="sm" onClick={() => setConfirmKill(true)}>
          <SkullIcon />
          Kill session
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          pending={resume.isPending}
          onClick={() => {
            resume.mutate(session.id, {
              onSuccess: (revived) => {
                // The daemon answers with this same session either way: live
                // again when it really relaunched it, or untouched when its
                // pane turned out to be alive after all (the scheduler may
                // have respawned it already) — which its status is what says.
                if (!isLiveStatus(revived.status)) {
                  toast.info("That pane is already alive", {
                    description: `${revived.tmux_session} has a running agent; nothing to resume.`,
                  })
                  return
                }
                toast.success("Session resumed", {
                  description: `${revived.tmux_session} · same agent conversation`,
                })
                onResumed?.(revived)
              },
              onError: (error) =>
                toast.error("Could not resume", { description: describeError(error) }),
            })
          }}
        >
          <PlayIcon />
          Resume session
        </Button>
      )}

      <ConfirmDialog
        open={confirmKill}
        onClose={() => {
          setConfirmKill(false)
          kill.reset()
        }}
        title="Kill this session?"
        description={
          <>
            The agent's tmux process (<code className="font-mono">{session.tmux_session}</code>) is
            terminated wherever it got to. Its conversation is kept, so the session can be resumed
            afterwards.
          </>
        }
        confirmLabel="Kill session"
        destructive
        pending={kill.isPending}
        // The kill is confirmed in a dialog, so its refusal is read there
        // rather than toasted from behind it — and the rolled-back status is
        // then next to the reason it rolled back.
        error={kill.error}
        errorTitle="Could not kill the session"
        onConfirm={() => {
          kill.mutate(session.id, {
            onSuccess: () => {
              setConfirmKill(false)
              // What was done, like every other success toast — not the status
              // it left behind, which the badge on the row already says.
              toast.success("Session killed", {
                description: `${session.tmux_session} · the conversation is kept, so it can be resumed`,
              })
            },
          })
        }}
      />
    </div>
  )
}
