/**
 * Kill and resume, the two things the UI can do *to* a session.
 *
 * Both are named with what they act on, like the goal's and the task's: these
 * sit in the same corner of a panel that is itself dismissible, and a bare
 * verb there reads as being about the panel.
 *
 * Kill is destructive and irreversible — it stops the agent's process
 * mid-thought — so it asks first, and because it asks, its refusal is
 * shown in that dialog like every other confirmed action's. Resume is not
 * destructive and has no dialog to put anything in; it also fails often and
 * for reasons worth reading (the daemon answers `409` when the session has no
 * agent-internal id to resume from, or is still running), so it toasts. See
 * `components/ui/sonner.tsx` for the rule those two are an instance of.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { ArrowRightLeftIcon, PlayIcon, SkullIcon } from "lucide-react"
import { useState } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"
import type { SessionDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useResetOnOpen,
} from "@/components/form-dialog"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { modelRefField } from "@/features/models/model-ref"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { describeError, shortId } from "@/lib/format"

import { useKillSession, useResumeSession, useSwitchSession } from "./queries"
import { isLiveStatus } from "./session-display"

const switchSchema = z.object({ model: modelRefField(), effort: z.string() })
type SwitchForm = z.infer<typeof switchSchema>

/**
 * Resume a session that ended, with the toasts the panel's button gives. The
 * daemon answers with this same session either way: live again when it really
 * relaunched it, or untouched when its agent turned out to be alive after all
 * (the scheduler may have respawned it already) — which its status says.
 */
export function useResumeAction(onResumed?: (session: SessionDto) => void) {
  const resume = useResumeSession()
  return {
    pending: resume.isPending,
    resume(id: string) {
      resume.mutate(id, {
        onSuccess: (revived) => {
          if (!isLiveStatus(revived.status)) {
            toast.info("That agent is already alive", {
              description: `${shortId(revived.id)} has a running agent; nothing to resume.`,
            })
            return
          }
          toast.success("Session resumed", {
            description: `${shortId(revived.id)} · same agent conversation`,
          })
          onResumed?.(revived)
        },
        onError: (error) => toast.error("Could not resume", { description: describeError(error) }),
      })
    },
  }
}

/** The dialog that starts a session's successor on another agent or model. */
export function SwitchSessionDialog({
  session,
  open,
  onOpenChange,
  onSwitched,
}: {
  session: SessionDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onSwitched?: (session: SessionDto) => void
}) {
  const switched = useSwitchSession()
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
  const form = useForm<SwitchForm>({
    resolver: zodResolver(switchSchema),
    defaultValues: { model: session.model, effort: session.effort ?? "" },
  })
  useResetOnOpen(open, form, { model: session.model, effort: session.effort ?? "" }, switched)
  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        title="Switch session"
        description="Start this session's successor on another model or agent."
        onSubmit={form.handleSubmit((values) => {
          const effort = values.effort.trim()
          switched.mutate(
            { id: session.id, model: values.model.trim(), ...(effort ? { effort } : {}) },
            {
              onSuccess: (next) => {
                onOpenChange(false)
                toast.success("Session switched", { description: next.model })
                if (next.id !== session.id) onSwitched?.(next)
              },
              onError: (error) =>
                toast.error("Could not switch session", { description: describeError(error) }),
            },
          )
        })}
        submitLabel="Switch"
        pending={switched.isPending}
        submitDisabled={form.watch("model").trim().length === 0}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <Field data-invalid={form.formState.errors.model ? "" : undefined}>
            <FieldLabel htmlFor="switch-session-pin">Runs on</FieldLabel>
            <Controller
              control={form.control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="switch-session-pin"
                  label="Runs on"
                  model={field.value}
                  effort={form.watch("effort")}
                  models={models.data}
                  invalid={Boolean(form.formState.errors.model)}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    form.setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                />
              )}
            />
            {form.formState.errors.model ? (
              <FieldError>{form.formState.errors.model.message}</FieldError>
            ) : (
              <FieldDescription>
                The agent and, after a <code>:</code>, the model of it.
              </FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}

export function SessionActions({
  session,
  onResumed,
  onSwitched,
  goalCancelled = false,
}: {
  session: SessionDto
  /**
   * Handed the revived session — this same one, relaunched. Where to go from
   * there is the caller's call — a panel selects it — so these buttons stay
   * usable wherever they are rendered.
   */
  onResumed?: (session: SessionDto) => void
  onSwitched?: (session: SessionDto) => void
  goalCancelled?: boolean
}) {
  const [confirmKill, setConfirmKill] = useState(false)
  const [switchOpen, setSwitchOpen] = useState(false)
  const kill = useKillSession()
  const resume = useResumeAction(onResumed)
  const live = isLiveStatus(session.status)

  return (
    <div className="flex items-center gap-2">
      {!goalCancelled ? (
        <Button variant="outline" size="sm" onClick={() => setSwitchOpen(true)}>
          <ArrowRightLeftIcon />
          Switch
        </Button>
      ) : null}
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
          pending={resume.pending}
          onClick={() => resume.resume(session.id)}
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
            The agent's process (<code className="font-mono">{shortId(session.id)}</code>) is
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
                description: `${shortId(session.id)} · the conversation is kept, so it can be resumed`,
              })
            },
          })
        }}
      />
      <SwitchSessionDialog
        session={session}
        open={switchOpen}
        onOpenChange={setSwitchOpen}
        onSwitched={onSwitched}
      />
    </div>
  )
}
