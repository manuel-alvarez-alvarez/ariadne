/**
 * Kill and resume, the two things the UI can do *to* a session.
 *
 * Kill is destructive and irreversible — it tears down the agent's tmux
 * process mid-thought — so it asks first. Resume is not destructive, but it
 * fails often and for reasons worth reading (the daemon answers `409` when the
 * session has no agent-internal id to resume from, or is still running), so
 * its error envelope is put on screen rather than swallowed.
 */

import { PlayIcon, SkullIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type { SessionDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Button } from "@/components/ui/button"
import { describeError } from "@/lib/errors"

import { useKillSession, useResumeSession } from "./queries"
import { isLiveStatus, sessionStatusLabel } from "./session-display"

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
        <Button
          variant="destructive"
          size="sm"
          disabled={kill.isPending}
          onClick={() => setConfirmKill(true)}
        >
          <SkullIcon />
          Kill
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          disabled={resume.isPending}
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
          {resume.isPending ? "Resuming…" : "Resume"}
        </Button>
      )}

      <ConfirmDialog
        open={confirmKill}
        onClose={() => setConfirmKill(false)}
        title="Kill this session?"
        description={
          <>
            The agent's tmux process (<code className="font-mono">{session.tmux_session}</code>) is
            terminated wherever it got to. Its conversation is kept, so the session can be resumed
            afterwards.
          </>
        }
        confirmLabel="Kill session"
        pendingLabel="Killing…"
        destructive
        pending={kill.isPending}
        onConfirm={() => {
          kill.mutate(session.id, {
            onSuccess: (killed) => {
              setConfirmKill(false)
              toast.success(`Session is now ${sessionStatusLabel(killed.status).toLowerCase()}`)
            },
            onError: (error) =>
              toast.error("Could not kill", { description: describeError(error) }),
          })
        }}
      />
    </div>
  )
}
