import { useEffect, useState } from "react"
import { useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type WorkflowDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  useResetOnOpen,
} from "@/components/form-dialog"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { describeError } from "@/lib/format"

import { useCreateWorkflow } from "./queries"

const TEMPLATE = `workflow develop-review-merge
  develop[Develop]
    Build the task on its branch and commit it.
    skills: coding
    rank: balanced
    gate: committed
  review[Review]
    Run the whole suite and judge the change against the task and the repository rules.
    Fail the step with the changes to make.
    skills: code-review
    rank: frontier
  merge[Merge]
    Rebase onto the base branch, run the whole suite, squash, fast-forward and push.
    skills: merge
    rank: fast
    gate: merged`
const EMPTY = { name: "", document: "" }

export function CreateWorkflowDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (workflow: WorkflowDto) => void
}) {
  const create = useCreateWorkflow()
  const form = useForm({ defaultValues: EMPTY })
  const { formState, handleSubmit, register, setError, setValue, watch } = form
  useResetOnOpen(open, form, EMPTY, create)
  const [touched, setTouched] = useState(false)
  const name = watch("name")
  useEffect(() => {
    if (!touched)
      setValue("document", TEMPLATE.replace("develop-review-merge", name.trim() || "my-workflow"))
  }, [name, setValue, touched])
  useEffect(() => {
    if (open) setTouched(false)
  }, [open])
  async function submit(values: typeof EMPTY) {
    const name = values.name.trim()
    try {
      const workflow = await create.mutateAsync({ name, document: values.document })
      toast.success("Workflow created", { description: workflow.name })
      onOpenChange(false)
      onCreated?.(workflow)
    } catch (error) {
      toast.error("Could not create workflow", { description: describeError(error) })
      if (ApiError.is(error) && error.status === 409)
        setError("name", { message: `A workflow named "${name}" already exists.` })
      else setError("root", { message: describeError(error) })
    }
  }
  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        title="New workflow"
        description="A sequence of columns that stages work through a task."
        onSubmit={handleSubmit(submit)}
        submitLabel="Create workflow"
        pending={create.isPending}
        error={
          formState.errors.root
            ? {
                title: "Could not create the workflow",
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="new-workflow-name">Name</FieldLabel>
            <Input
              id="new-workflow-name"
              autoComplete="off"
              {...register("name", { required: "A workflow needs a name." })}
            />
            {formState.errors.name ? <FieldError errors={[formState.errors.name]} /> : null}
          </Field>
          <Field>
            <FieldLabel htmlFor="new-workflow-document">Document</FieldLabel>
            <Textarea
              id="new-workflow-document"
              rows={14}
              spellCheck={false}
              className="resize-none font-mono text-xs"
              {...register("document", { onChange: () => setTouched(true) })}
            />
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
