import { useEffect, useRef, useState } from "react"
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
import { describeError } from "@/lib/format"

import { useCreateWorkflow } from "./queries"
import { useWorkflowEditing } from "./use-workflow-editing"
import { WorkflowCodeEditor } from "./workflow-code-editor"
import { WorkflowPreview } from "./workflow-preview"

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
const EMPTY = { name: "" }

function template(name: string): string {
  return TEMPLATE.replace("develop-review-merge", name.trim() || "my-workflow")
}

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
  const { formState, handleSubmit, register, setError, watch } = form
  useResetOnOpen(open, form, EMPTY, create)
  const lastTemplate = useRef(template(""))
  const [document, setDocument] = useState(lastTemplate.current)
  const [touched, setTouched] = useState(false)
  const name = watch("name")
  useEffect(() => {
    if (touched) return
    lastTemplate.current = template(name)
    setDocument(lastTemplate.current)
  }, [name, touched])
  useEffect(() => {
    if (open) {
      setTouched(false)
      lastTemplate.current = template("")
      setDocument(lastTemplate.current)
    }
  }, [open])
  const { parsed, error, extensions } = useWorkflowEditing(document)

  async function submit(values: typeof EMPTY) {
    const name = values.name.trim()
    try {
      const workflow = await create.mutateAsync({ name, document })
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
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty || touched}>
      <FormDialogContent
        title="New workflow"
        description="A sequence of columns that stages work through a task."
        onSubmit={handleSubmit(submit)}
        submitLabel="Create workflow"
        pending={create.isPending}
        className="sm:max-w-4xl"
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
          <div className="grid gap-4 sm:grid-cols-2">
            <section aria-label="Workflow editor" className="flex min-h-0 flex-col gap-4">
              <Field data-invalid={formState.errors.name ? true : undefined}>
                <FieldLabel htmlFor="new-workflow-name">Name</FieldLabel>
                <Input
                  id="new-workflow-name"
                  autoComplete="off"
                  {...register("name", { required: "A workflow needs a name." })}
                />
                {formState.errors.name ? <FieldError errors={[formState.errors.name]} /> : null}
              </Field>
              <Field className="flex min-h-0 flex-1 flex-col">
                <FieldLabel id="new-workflow-document-label" htmlFor="new-workflow-document">
                  Document
                </FieldLabel>
                <div className="flex h-72 flex-col">
                  <WorkflowCodeEditor
                    id="new-workflow-document"
                    labelId="new-workflow-document-label"
                    value={document}
                    onChange={(value) => {
                      setDocument(value)
                      if (value !== lastTemplate.current) setTouched(true)
                    }}
                    error={error}
                    extensions={extensions}
                  />
                </div>
              </Field>
            </section>
            <WorkflowPreview parsed={parsed} />
          </div>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
