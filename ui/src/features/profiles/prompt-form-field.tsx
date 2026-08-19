/**
 * One prompt inside the profile form: a collapsible section holding a plain
 * form field.
 *
 * Nothing is written until the form is submitted, so there is no save and no
 * failure of its own — only text, and a "restore default" that fills the box
 * from the role's defaults and leaves the writing to the submit.
 *
 * A profile carries up to four prompts and every one of them is long, so they
 * are folded away by default. Collapsed sections are unmounted; their text
 * lives in the form's state, not in the textarea, so folding one shut keeps
 * whatever was typed into it.
 */

import { ChevronDownIcon, ChevronRightIcon, RotateCcwIcon } from "lucide-react"
import { type ReactNode, useId } from "react"

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
   * The role default for this prompt, or undefined while it is unknown — the
   * defaults endpoint is allowed to fail, and then there is nothing to restore
   * to and nothing to call the text customised against.
   */
  defaultContent: string | undefined
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
  defaultContent,
  open,
  onOpenChange,
  error,
  placeholder,
}: PromptFormFieldProps) {
  const fieldId = useId()
  const customised = defaultContent !== undefined && value !== defaultContent

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
        {customised ? (
          <Badge variant="outline" className="shrink-0">
            edited
          </Badge>
        ) : null}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="shrink-0"
          aria-label={`Restore ${label} to default`}
          // Nothing to restore to: the role defaults never arrived.
          disabled={defaultContent === undefined || !customised}
          onClick={() => onChange(defaultContent ?? "")}
        >
          <RotateCcwIcon />
          Restore default
        </Button>
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
    </section>
  )
}
