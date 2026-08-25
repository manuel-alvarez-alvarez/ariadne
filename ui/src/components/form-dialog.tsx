/**
 * A dialog that will not throw away what was typed into it.
 *
 * Every dialog in the app dismisses on an outside press and on Escape, which
 * is right for the ones that only show something and wrong for the ones that
 * hold a form: a goal brief or a profile's four prompts are the longest text
 * the app ever asks for, and one misplaced click a few paragraphs in is not an
 * instruction to delete them.
 *
 * So a form dialog says whether it is dirty, and this stands in for
 * `<Dialog>`: pristine it dismisses exactly as before, dirty every way out —
 * outside press, Escape, the close button, the form's own Cancel — asks
 * first.
 *
 * What it guards is dismissal, which is the dialog root's `onOpenChange` and
 * nothing else. A submit is not a dismissal and needs no way around the guard:
 * a form closes itself on success by calling the `onOpenChange` it was handed,
 * which is the caller's own setter rather than the guarded handler this passes
 * down, so a saved form closes on the spot with nothing asked about a draft
 * that is no longer one. `form-dialog.test.tsx` pins that, as does the
 * repository dialog's own test through a real mutation.
 *
 * The confirmation is a dialog of its own nested inside this one's root, which
 * is what keeps Base UI's stacking, focus trap and Escape handling straight:
 * Escape over the confirmation answers it rather than the form behind it.
 */

import { type FormEvent, type ReactNode, useEffect, useRef, useState } from "react"
import type { FieldValues, UseFormReturn } from "react-hook-form"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { ErrorState } from "@/components/error-state"
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

export function FormDialog({
  open,
  onOpenChange,
  dirty,
  discardTitle = "Discard changes?",
  discardDescription = "This form has unsaved changes. Closing it now drops them.",
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Whether the form holds anything a dismissal would destroy. */
  dirty: boolean
  discardTitle?: string
  discardDescription?: ReactNode
  children: ReactNode
}) {
  const [asking, setAsking] = useState(false)

  // The form closing some other way — a successful submit, the screen behind it
  // navigating — takes the question with it, so the next open does not come up
  // with a discard prompt over a form nobody has touched yet.
  useEffect(() => {
    if (!open) setAsking(false)
  }, [open])

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // Opening, and closing a pristine form, are what they have always been.
        if (next || !dirty) {
          setAsking(false)
          onOpenChange(next)
          return
        }
        // Dirty: the dialog is controlled, so not forwarding this is what keeps
        // it on screen with everything in it intact.
        setAsking(true)
      }}
    >
      {children}
      <ConfirmDialog
        open={asking}
        onClose={() => setAsking(false)}
        title={discardTitle}
        description={discardDescription}
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => {
          setAsking(false)
          onOpenChange(false)
        }}
      />
    </Dialog>
  )
}

/**
 * The shape every form in the app has inside that guard: a titled header, the
 * fields, whatever the daemon refused with, and a Cancel/submit pair.
 *
 * All four form dialogs — goal, task, profile, repository — were spelling this
 * out for themselves, which is how their *Cancel* buttons came to disagree:
 * the profile's and the repository's went out of reach while a save was in
 * flight, the goal's and the task's did not. They all do now. The fields are
 * the caller's; everything around them is here.
 *
 * A submit is not a dismissal: `onSubmit` is the form's own handler, and a form
 * that saved closes by calling the `onOpenChange` its caller holds rather than
 * anything below.
 */
export function FormDialogContent({
  title,
  description,
  onSubmit,
  error,
  submitLabel,
  pending,
  className,
  children,
}: {
  title: ReactNode
  description: ReactNode
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  /** What the daemon refused with, as the alert above the buttons. */
  error?: { title: string; error: unknown; description?: ReactNode; showIcon?: boolean } | null
  submitLabel: ReactNode
  /** The submit is spinning and both buttons are out of reach. */
  pending: boolean
  /** Sizing for the dialog box; forms differ in how much they have to hold. */
  className?: string
  children: ReactNode
}) {
  return (
    <DialogContent className={className}>
      <form onSubmit={onSubmit} className="grid gap-4">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        {children}
        {error ? (
          <ErrorState
            showIcon={error.showIcon}
            title={error.title}
            error={error.error}
            description={error.description}
          />
        ) : null}
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" disabled={pending} />}>
            Cancel
          </DialogClose>
          <Button type="submit" pending={pending}>
            {submitLabel}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}

/**
 * Re-opening a form starts from a clean slate — the row as it stands, or blank
 * — never from the previous attempt, and never from a draft the last open left
 * behind.
 *
 * The defaults go through a ref so only *opening* resets: an event off the
 * stream that re-renders the dialog mid-edit must not wipe what was typed.
 */
export function useResetOnOpen<V extends FieldValues>(
  open: boolean,
  form: UseFormReturn<V>,
  defaultValues: V,
  /** The mutation whose last failure should not greet the next open. */
  mutation: { reset: () => void },
): void {
  const latest = useRef(defaultValues)
  latest.current = defaultValues
  const { reset } = form
  const mutationReset = mutation.reset
  useEffect(() => {
    if (!open) return
    reset(latest.current)
    mutationReset()
  }, [open, reset, mutationReset])
}

/**
 * A daemon refusal that is about the values on screen — "goal already has 3 of
 * max 3 tasks", "repo path does not exist" — must not still be up once one of
 * them has changed, so the first edit after a failure drops the alert.
 */
export function useClearErrorOnEdit<V extends FieldValues>(
  form: UseFormReturn<V>,
  mutation: { error: unknown; reset: () => void },
): void {
  const { watch } = form
  const { error, reset } = mutation
  useEffect(() => {
    if (!error) return
    const subscription = watch(() => reset())
    return () => subscription.unsubscribe()
  }, [error, watch, reset])
}
