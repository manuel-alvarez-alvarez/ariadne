/**
 * The pieces the host dialogs share: the colour picker, and the field a
 * refusal of `POST` or `PATCH /v1/hosts` is about.
 */

import { useId } from "react"

import { ApiError } from "@/api"
import { HostColorSwatch } from "@/components/host-color-swatch"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { cn } from "@/lib/format"
import { HOST_COLORS, type HostColor } from "@/lib/host-colors"

/** A field of a host form that the daemon can name in a refusal. */
export type HostField = "name" | "endpoint" | "remote" | "color"

/** The field a refusal is about, from the words the daemon names it in. */
export function fieldOf(error: unknown): HostField | null {
  if (!ApiError.is(error)) return null
  if (error.status !== 400 && error.status !== 409) return null
  if (/remote/i.test(error.message)) return "remote"
  if (/endpoint/i.test(error.message)) return "endpoint"
  if (/name/i.test(error.message)) return "name"
  if (/colou?r/i.test(error.message)) return "color"
  return null
}

/**
 * "Default" and the eight colours of `HOST_COLORS`, as swatches. "" is the
 * default colour, which the caller sends as `null`.
 */
export function HostColorField({
  value,
  onChange,
  error,
  description = "Default draws the host in the colour its name gets.",
}: {
  value: HostColor | ""
  onChange: (value: HostColor | "") => void
  error?: { message?: string }
  description?: string
}) {
  const labelId = useId()
  return (
    <Field data-invalid={error ? true : undefined}>
      <FieldLabel id={labelId}>Colour</FieldLabel>
      <div role="radiogroup" aria-labelledby={labelId} className="flex flex-wrap gap-2">
        {[{ value: "" as const, label: "Default" }, ...HOST_COLORS].map((option) => (
          <label
            key={option.value || "default"}
            className={cn(
              "inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 text-sm has-focus-visible:ring-3 has-focus-visible:ring-ring/50",
              value === option.value
                ? "border-foreground bg-accent"
                : "border-input text-muted-foreground",
            )}
          >
            <input
              type="radio"
              name={labelId}
              className="sr-only"
              checked={value === option.value}
              onChange={() => onChange(option.value)}
            />
            {option.value ? <HostColorSwatch color={option.value} /> : null}
            {option.label}
          </label>
        ))}
      </div>
      {error ? <FieldError errors={[error]} /> : <FieldDescription>{description}</FieldDescription>}
    </Field>
  )
}
