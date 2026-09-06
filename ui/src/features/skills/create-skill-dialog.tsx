/**
 * Writing a skill of your own.
 *
 * Two fields, because a skill is two things: the name an agent loads it by,
 * and the document. Everything else a skill says about itself — the one line
 * a listing and an agent's index show — is read off the document's own
 * frontmatter, so there is no third field to keep in step with it.
 *
 * Validation is client-side for the two required fields and daemon-side for
 * what it alone knows: a name already taken comes back as a 409 and is shown
 * on the name field.
 */

import { useEffect, useState } from "react"
import { useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type SkillDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  useResetOnOpen,
} from "@/components/form-dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { describeError } from "@/lib/format"

import { useCreateSkill } from "./queries"

type SkillFormValues = { name: string; document: string }

const EMPTY: SkillFormValues = { name: "", document: "" }

/** What a new skill opens on: the frontmatter every skill starts with. */
function skillTemplate(name: string): string {
  return `---\nname: ${name || "my-skill"}\ndescription: \n---\n\n# \n\n`
}

export function CreateSkillDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** What the screen does with the new skill once the dialog has closed. */
  onCreated?: (skill: SkillDto) => void
}) {
  const createSkill = useCreateSkill()

  const form = useForm<SkillFormValues>({ defaultValues: EMPTY })
  const { formState, handleSubmit, register, setError, setValue, watch } = form
  useResetOnOpen(open, form, EMPTY, createSkill)

  // The template follows the name until the document is touched, so the
  // frontmatter names the skill without anybody typing it twice. Written
  // through the form rather than over the box: the box is the form's, and a
  // value prop laid on top of it is dropped the moment the field registers.
  const name = watch("name")
  const [touched, setTouched] = useState(false)
  useEffect(() => {
    if (!touched) setValue("document", skillTemplate(name.trim()))
  }, [name, touched, setValue])
  // Reopening the dialog is a new skill, so the template follows the name
  // again — `useResetOnOpen` puts the fields back, and this puts back the
  // reason they are still the template's.
  useEffect(() => {
    if (open) setTouched(false)
  }, [open])

  async function submit(values: SkillFormValues) {
    const named = values.name.trim()
    const body = values.document || skillTemplate(named)
    try {
      const created = await createSkill.mutateAsync({ name: named, document: body })
      toast.success("Skill created", { description: created.name })
      onOpenChange(false)
      onCreated?.(created)
    } catch (error) {
      if (ApiError.is(error) && error.status === 409) {
        setError("name", { message: `A skill named "${named}" already exists.` })
        return
      }
      setError("root", { message: describeError(error) })
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        title="New skill"
        description="One document telling a generic agent how to do one kind of work. A task gives it to the agents that need it."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: "Could not create the skill",
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel="Create skill"
        pending={createSkill.isPending}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="new-skill-name">Name</FieldLabel>
            <Input
              id="new-skill-name"
              placeholder="api-design"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={formState.errors.name ? true : undefined}
              {...register("name", { required: "A skill needs a name." })}
            />
            {formState.errors.name ? (
              <FieldError errors={[formState.errors.name]} />
            ) : (
              <FieldDescription>
                Kebab-case: how an agent loads it, and how a task names it.
              </FieldDescription>
            )}
          </Field>

          <Field>
            <FieldLabel htmlFor="new-skill-document">Document</FieldLabel>
            <Textarea
              id="new-skill-document"
              rows={12}
              spellCheck={false}
              className="resize-none font-mono text-xs"
              {...register("document", { onChange: () => setTouched(true) })}
            />
            <FieldDescription>
              The whole <code>SKILL.md</code>. The <code>description</code> in its frontmatter is
              the line an agent reads before it opens the rest.
            </FieldDescription>
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
