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

import { ApiError, type SessionDto } from "@/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

import { useKillSession, useResumeSession } from "./queries"
import { isLiveStatus } from "./session-display"

export function SessionActions({
  session,
  onResumed,
}: {
  session: SessionDto
  /**
   * Handed the session a resume revived this one into, when that is a new
   * one. Where to go from there is the caller's call — the session screen
   * navigates to it, a panel selects it — so these buttons stay usable
   * wherever they are rendered.
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
                // The daemon answers with the session to attach to: a new one
                // carrying the same agent conversation when it really revived,
                // or this one unchanged when its pane turned out to be alive
                // after all (the scheduler may have respawned it already).
                if (revived.id === session.id) {
                  toast.info("That pane is already alive", {
                    description: `${revived.tmux_session} has a running agent; nothing to resume.`,
                  })
                  return
                }
                toast.success("Resumed as a new session", {
                  description: `${revived.tmux_session} · same agent conversation`,
                })
                onResumed?.(revived)
              },
              onError: (error) => toast.error("Could not resume", { description: reason(error) }),
            })
          }}
        >
          <PlayIcon />
          {resume.isPending ? "Resuming…" : "Resume"}
        </Button>
      )}

      <Dialog open={confirmKill} onOpenChange={setConfirmKill}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Kill this session?</DialogTitle>
            <DialogDescription>
              The agent's tmux process (<code className="font-mono">{session.tmux_session}</code>)
              is terminated wherever it got to. Its conversation is kept, so the session can be
              resumed afterwards.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button
              variant="destructive"
              disabled={kill.isPending}
              onClick={() => {
                kill.mutate(session.id, {
                  onSuccess: (killed) => {
                    setConfirmKill(false)
                    toast.success(`Session is now ${killed.status}`)
                  },
                  onError: (error) => toast.error("Could not kill", { description: reason(error) }),
                })
              }}
            >
              {kill.isPending ? "Killing…" : "Kill session"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

/** The daemon's own words, with its error code when it sent one. */
export function reason(error: unknown): string {
  if (ApiError.is(error)) {
    return error.code === "http_error" || error.isNetworkError
      ? error.message
      : `${error.message} (${error.code})`
  }
  return error instanceof Error ? error.message : String(error)
}
