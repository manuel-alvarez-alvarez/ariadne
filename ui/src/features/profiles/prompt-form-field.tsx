/**
 * One prompt inside the profile form: a collapsible section holding a plain
 * form field.
 *
 * Its text is written when the form is submitted, so there is no save of its
 * own. "Restore default" is the exception: a default is the daemon's text and
 * this form holds no copy of it, so restoring one is a write of its own — the
 * caller sends it and fills the box from what comes back. That is also why the
 * badge and the button read the *stored* state rather than what is in the box:
 * they are about what the daemon holds, which is what a restore acts on.
 *
 * And it is why it asks first. Every other control in this dialog is undone by
 * closing it; this one is not, so the question says the write happens now and
 * that dismissing the form afterwards will not take it back.
 *
 * A profile carries up to five prompts and every one of them is long, so they
 * are folded away by default. Collapsed sections are unmounted; their text
 * lives in the form's state, not in the textarea, so folding one shut keeps
 * whatever was typed into it.
 */

import { ChevronDownIcon, ChevronRightIcon, Undo2Icon } from "lucide-react"
import { type ReactNode, useId, useState } from "react"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { FieldDescription, FieldError } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"

interface PromptFormFieldProps {
  /** How the prompt is spelled on screen; also names the textarea. */
  label: string
  /** When the daemon sends this prompt, under the box. */
  hint: ReactNode
  value: string
  onChange: (content: string) => void
  /**
   * Whether the profile runs on the default of this prompt rather than on a
   * text of its own — the daemon's word, not a comparison made here.
   */
  isDefault: boolean
  /** Put the prompt back on its default. Absent = nothing to restore to. */
  onReset?: () => void
  /** True while that restore is in flight. */
  resetting?: boolean
  open: boolean
  onOpenChange: (open: boolean) => void
  /** A client-side validation message, shown under the box. */
  error?: string
  placeholder?: string
}

export function PromptFormField({
  label,
  hint,
  value,
  onChange,
  isDefault,
  onReset,
  resetting,
  open,
  onOpenChange,
  error,
  placeholder,
}: PromptFormFieldProps) {
  const fieldId = useId()
  const [confirming, setConfirming] = useState(false)

  return (
    <section
      className="overflow-hidden rounded-lg border"
      data-invalid={error ? true : undefined}
      data-slot="prompt-form-field"
    >
      <header className="flex items-center gap-2 bg-muted/40 px-2 py-1.5">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="-mx-1 min-w-0 flex-1 justify-start font-medium"
          aria-expanded={open}
          aria-label={open ? `Collapse ${label}` : `Expand ${label}`}
          onClick={() => onOpenChange(!open)}
        >
          {open ? <ChevronDownIcon /> : <ChevronRightIcon />}
          <span className="truncate">{label}</span>
        </Button>
        <Badge variant="outline" className="shrink-0">
          {isDefault ? "default" : "edited"}
        </Badge>
        {onReset ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="shrink-0"
            aria-label={`Restore ${label} to default`}
            // Already on its default: there is nothing to drop.
            disabled={isDefault || resetting}
            onClick={() => setConfirming(true)}
          >
            <Undo2Icon />
            Restore default
          </Button>
        ) : null}
      </header>

      {open ? (
        <div className="flex flex-col gap-2 p-2">
          <Textarea
            id={fieldId}
            aria-label={label}
            aria-invalid={error ? true : undefined}
            spellCheck={false}
            className="max-h-96 min-h-56 resize-y font-mono text-xs leading-relaxed"
            placeholder={placeholder}
            value={value}
            onChange={(event) => onChange(event.target.value)}
          />
          {error ? (
            <FieldError errors={[{ message: error }]} />
          ) : (
            <FieldDescription className="text-xs">{hint}</FieldDescription>
          )}
        </div>
      ) : null}

      {/* The one control in this form that acts before the form is submitted,
          so it is the one that asks — and the question says exactly that. The
          dialog goes on the confirming click: what the restore may fail with is
          reported by the caller, in the alert the rest of this dialog's
          failures already land in. */}
      <ConfirmDialog
        open={confirming}
        onClose={() => setConfirming(false)}
        title={`Restore ${label.toLowerCase()} to its default?`}
        description={
          <>
            The text this profile has of its own is dropped and the{" "}
            <span className="font-medium text-foreground">{label.toLowerCase()}</span> goes back to
            the default of its role. It is written straight away: closing the form without saving
            does not put it back.
          </>
        }
        confirmLabel="Restore default"
        destructive
        onConfirm={() => {
          setConfirming(false)
          onReset?.()
        }}
      />
    </section>
  )
}
