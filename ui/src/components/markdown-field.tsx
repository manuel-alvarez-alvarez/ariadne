/**
 * The brief: the one field in a form that holds paragraphs rather than a line,
 * and the Markdown they will be read as.
 *
 * A goal's description is what the planner works from and a task's is what the
 * engineer builds from — headings, lists, fenced code, the longest prose the
 * app ever asks anyone for — so it gets a box of that size. Ten lines to start
 * on, a grip to drag it taller, and the rendered text beside it through the
 * app's one renderer (`@/components/markdown`), which is exactly what the
 * panels will show once it is saved.
 *
 * Preview is a *view* of the value, never a second copy of it: the field is
 * controlled from outside, so switching back finds the text as it was left.
 */

import { type ChangeEvent, type ReactNode, type Ref, useState } from "react"

import { Markdown } from "@/components/markdown"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Textarea } from "@/components/ui/textarea"

type Mode = "write" | "preview"

/**
 * What the box opens at, and what the preview is floored to so that switching
 * between them moves nothing: ten lines of the text they both hold, plus the
 * padding and border they share.
 */
const TEN_LINES = "min-h-56"

export function MarkdownField({
  id,
  label,
  description,
  placeholder,
  value,
  onChange,
  onBlur,
  name,
  ref,
}: {
  /** What the label points at, so the box is found by its name. */
  id: string
  label: ReactNode
  /** The line under the box; the field's own note about what it is for. */
  description?: ReactNode
  placeholder?: string
  value: string
  onChange: (event: ChangeEvent<HTMLTextAreaElement>) => void
  onBlur?: () => void
  name?: string
  ref?: Ref<HTMLTextAreaElement>
}) {
  const [mode, setMode] = useState<Mode>("write")

  return (
    <Field>
      <Tabs value={mode} onValueChange={(next) => setMode(next as Mode)}>
        <div className="flex items-center justify-between gap-2">
          <FieldLabel htmlFor={id}>{label}</FieldLabel>
          <TabsList>
            <TabsTrigger value="write">Write</TabsTrigger>
            <TabsTrigger value="preview">Preview</TabsTrigger>
          </TabsList>
        </div>
        <TabsContent value="write">
          {/* `field-sizing-fixed` undoes the base textarea's grow-with-content:
              a box that sizes itself cannot also be dragged to a size. */}
          <Textarea
            id={id}
            name={name}
            ref={ref}
            value={value}
            onChange={onChange}
            onBlur={onBlur}
            placeholder={placeholder}
            className={`${TEN_LINES} field-sizing-fixed resize-y`}
          />
        </TabsContent>
        <TabsContent value="preview">
          <div className={`${TEN_LINES} rounded-lg border px-2.5 py-2`}>
            {value.trim().length > 0 ? (
              <Markdown>{value}</Markdown>
            ) : (
              <p className="text-sm text-muted-foreground">Nothing written yet.</p>
            )}
          </div>
        </TabsContent>
      </Tabs>
      {description ? <FieldDescription>{description}</FieldDescription> : null}
    </Field>
  )
}
