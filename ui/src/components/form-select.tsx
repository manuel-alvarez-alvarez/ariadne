/**
 * A form's select, wired to react-hook-form, with the options it offers.
 *
 * Six of these across four dialogs were each spelling out the same `Controller`
 * around the same trigger/content pair, and disagreeing on the details that
 * matter: the trigger has to be given `items` or it shows the raw value the
 * field holds — a profile id, not a profile's name — and an empty selection has
 * to be `null` going in and the empty string coming out, because that is what
 * the zod schemas validate against.
 *
 * The `Field` around it stays at the call site: some of these are one field of
 * a form and some are one row of a repeated list, next to the button that
 * removes the row.
 */

import type { ReactNode } from "react"
import type { Control, FieldPath, FieldValues } from "react-hook-form"
import { Controller } from "react-hook-form"

import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { cn } from "@/lib/format"

/** One choice: what the trigger and the list call it, and what is stored. */
interface SelectOption {
  label: string
  value: string
}

export function FormSelect<V extends FieldValues>({
  control,
  name,
  id,
  ariaLabel,
  options,
  placeholder,
  disabled,
  invalid,
  className,
  empty = "",
  renderOption,
}: {
  control: Control<V>
  name: FieldPath<V>
  /** For the fields a `<FieldLabel htmlFor>` points at. */
  id?: string
  /** For the rows of a repeated list, which have no label of their own. */
  ariaLabel?: string
  options: SelectOption[]
  placeholder?: string
  disabled?: boolean
  invalid?: boolean
  className?: string
  /** What clearing the select stores; the empty string unless a value is required. */
  empty?: string
  /** A richer item body than its label — a path in mono, a title beside its id. */
  renderOption?: (option: SelectOption) => ReactNode
}) {
  return (
    <Controller
      control={control}
      name={name}
      render={({ field }) => (
        <Select
          value={field.value || null}
          onValueChange={(value) => field.onChange(value ?? empty)}
          disabled={disabled}
          // Without this the trigger would show the stored value rather than
          // the option's label.
          items={options}
        >
          <SelectTrigger
            id={id}
            aria-label={ariaLabel}
            aria-invalid={invalid ? true : undefined}
            className={cn("w-full", className)}
          >
            <SelectValue placeholder={placeholder} />
          </SelectTrigger>
          <SelectContent>
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {renderOption ? renderOption(option) : option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    />
  )
}

/**
 * What a select says while it has nothing to offer: the profiles are loading,
 * the daemon refused, or this role has none registered. Every picker of a
 * profile had its own copy of this, worded the same.
 */
export function profilePlaceholder(
  query: { isPending: boolean; isError: boolean; data?: unknown[] },
  role: string,
): string {
  if (query.isPending) return "Loading…"
  if (query.isError) return "Profiles unavailable"
  if (!query.data?.length) return `No ${role} profiles`
  return "Select a profile"
}
