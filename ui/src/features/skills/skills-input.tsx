/**
 * One agent's skills, as a form control: comma-separated names, with the
 * catalog behind them as suggestions.
 *
 * Free text against a list rather than a closed select, because the catalog is
 * the user's to extend and a task may name a skill written a minute ago. The
 * daemon is what refuses a name nothing answers to; this only offers the ones
 * it knows and says so where a name is not among them.
 */

import type { Control, FieldPath, FieldValues } from "react-hook-form"
import { Controller } from "react-hook-form"

import { Input } from "@/components/ui/input"
import { cn } from "@/lib/format"

import { parseSkillNames } from "./parse"

export function SkillsInput<T extends FieldValues>({
  control,
  name,
  id,
  ariaLabel,
  suggestions,
  invalid,
  className,
}: {
  control: Control<T>
  name: FieldPath<T>
  id?: string
  ariaLabel?: string
  /** Every skill the daemon knows, offered as completions. */
  suggestions: string[]
  invalid?: boolean
  className?: string
}) {
  // One list per control id, so two rows on the same screen do not share one.
  const listId = `${id ?? name}-skills`

  return (
    <Controller
      control={control}
      name={name}
      render={({ field }) => {
        const named = parseSkillNames(String(field.value ?? ""))
        const unknown = named.filter((skill) => !suggestions.includes(skill))
        return (
          <div className={cn("flex min-w-0 flex-col gap-1", className)}>
            <Input
              id={id}
              list={listId}
              aria-label={ariaLabel}
              aria-invalid={invalid ? true : undefined}
              autoComplete="off"
              spellCheck={false}
              placeholder="coding, testing"
              value={String(field.value ?? "")}
              onChange={field.onChange}
              onBlur={field.onBlur}
              name={field.name}
              ref={field.ref}
            />
            <datalist id={listId}>
              {suggestions.map((skill) => (
                <option key={skill} value={skill} />
              ))}
            </datalist>
            {/* Said, not enforced: the catalog may simply not have loaded, and
                the daemon is the one that decides. */}
            {suggestions.length > 0 && unknown.length > 0 ? (
              <p className="text-muted-foreground text-xs">
                No skill named {unknown.map((skill) => `“${skill}”`).join(", ")}.
              </p>
            ) : null}
          </div>
        )
      }}
    />
  )
}
