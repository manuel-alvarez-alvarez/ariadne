/**
 * "Add memory": the user's own write, matching `ariadne memory add` (015).
 *
 * A memory is its text, its scope — global, or one repository — and an
 * optional expiry; one saved without an expiry never expires. A user write
 * records no source.
 *
 * The daemon alone decides whether a memory may be saved, so the client checks
 * only that there is text. A refusal stays above the buttons with the form as
 * it was typed, until the next edit.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import type { RepositoryDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { folderName } from "@/lib/format"

import { useCreateMemory } from "./queries"

/** The scope select's value for a global memory; a repository is its id. */
export const GLOBAL_SCOPE = "global"

const formSchema = z.object({
  text: z.string().trim().min(1, "Enter what to remember."),
  scope: z.string(),
  /** A `datetime-local` value, or empty for no expiry. */
  expires: z.string(),
})

type MemoryFormValues = z.infer<typeof formSchema>

export function AddMemoryDialog({
  open,
  onOpenChange,
  repositories,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** What the scope select offers beside global. */
  repositories: RepositoryDto[]
}) {
  const createMemory = useCreateMemory()
  const form = useForm<MemoryFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: { text: "", scope: GLOBAL_SCOPE, expires: "" },
  })
  const { formState, handleSubmit, register, control } = form
  useResetOnOpen(open, form, { text: "", scope: GLOBAL_SCOPE, expires: "" }, createMemory)
  useClearErrorOnEdit(form, createMemory)

  const scopes = [
    { label: "Global", value: GLOBAL_SCOPE },
    ...repositories.map((repository) => ({
      label: folderName(repository.path),
      value: repository.id,
    })),
  ]

  function submit(values: MemoryFormValues) {
    createMemory.mutate(
      {
        text: values.text.trim(),
        repository_id: values.scope === GLOBAL_SCOPE ? null : values.scope,
        expires_at: values.expires ? new Date(values.expires).toISOString() : null,
      },
      {
        onSuccess: () => {
          onOpenChange(false)
          toast.success("Memory saved")
        },
      },
    )
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-lg"
        title="Add memory"
        description="A fact an agent finds when it searches memory. Nothing puts it into a prompt."
        onSubmit={handleSubmit(submit)}
        onKeyDown={submitOnChord}
        error={
          createMemory.error
            ? { title: "Could not save the memory", error: createMemory.error }
            : null
        }
        submitLabel="Save memory"
        pending={createMemory.isPending}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.text ? true : undefined}>
            <FieldLabel htmlFor="memory-text">Text</FieldLabel>
            <Textarea
              id="memory-text"
              placeholder="The lint config lives in biome.json, not .eslintrc."
              aria-invalid={formState.errors.text ? true : undefined}
              {...register("text")}
            />
            {formState.errors.text ? <FieldError errors={[formState.errors.text]} /> : null}
          </Field>

          <Field>
            <FieldLabel htmlFor="memory-scope">Scope</FieldLabel>
            <FormSelect control={control} name="scope" id="memory-scope" options={scopes} />
            <FieldDescription>A global memory is found from every repository.</FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="memory-expires">Expires</FieldLabel>
            <Input id="memory-expires" type="datetime-local" {...register("expires")} />
            <FieldDescription>
              Optional. Leave it empty for a memory that never expires.
            </FieldDescription>
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
