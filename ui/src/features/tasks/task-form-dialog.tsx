import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { XIcon } from "lucide-react"
import { useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type GoalDto, type Host, type TaskDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { MarkdownField } from "@/components/markdown-field"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { goalQueryOptions } from "@/features/goals/queries"
import { RepositoryCombobox } from "@/features/goals/repository-combobox"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { skillsQueryOptions } from "@/features/skills/queries"
import { SkillsInput } from "@/features/skills/skills-input"
import { taskListQueryOptions, useCreateTask, useUpdateTask } from "./queries"
import {
  makeTaskFormSchema,
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
  toUpdateTaskRequest,
} from "./task-form-values"

export function CreateTaskDialog(props: {
  /** The host of the goal: the task is created there. */
  host: Host
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (task: TaskDto) => void
}) {
  return <TaskFormDialog {...props} />
}

export function EditTaskDialog(props: {
  host: Host
  task: TaskDto
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <TaskFormDialog
      host={props.host}
      editing={props.task}
      open={props.open}
      onOpenChange={props.onOpenChange}
    />
  )
}

function TaskFormDialog({
  host,
  goal,
  editing,
  open,
  onOpenChange,
  onCreated,
}: {
  host: Host
  goal?: GoalDto
  editing?: TaskDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (task: TaskDto) => void
}) {
  const goalId = goal?.id ?? editing?.goal_id ?? ""
  const detail = useQuery({
    ...goalQueryOptions(host, goalId),
    enabled: open && !goal && !editing?.execution_context,
  })
  const actualGoal = goal ?? detail.data
  const steps = editing?.execution_context?.workflow.steps ?? actualGoal?.steps ?? []
  const repositories = useQuery({ ...repositoriesQueryOptions(host), enabled: open && !!editing })
  const skills = useQuery({ ...skillsQueryOptions(), enabled: open })
  const models = useQuery({ ...modelsQueryOptions(host), enabled: open })
  const tasks = useQuery({ ...taskListQueryOptions(host, { goal: goalId }), enabled: open })
  const create = useCreateTask(host, goalId)
  const update = useUpdateTask(host, editing?.id ?? "")
  const submit = editing ? update : create
  const multiRepo = (actualGoal?.repos.length ?? 0) > 1
  const schema = useMemo(
    () => makeTaskFormSchema({ requireRepo: multiRepo && !editing }),
    [multiRepo, editing],
  )
  const defaults = useMemo(() => taskToFormValues(editing, steps), [editing, steps])
  const form = useForm<TaskFormValues>({ resolver: zodResolver(schema), defaultValues: defaults })
  const dependencies = useFieldArray({ control: form.control, name: "depends_on" })
  useResetOnOpen(open, form, defaults, submit)
  useClearErrorOnEdit(form, submit)
  const agentValues = form.watch("agents")
  const hasModel = agentValues.every((agent) => agent.model.trim())
  const skillNames = useMemo(
    () => (skills.data ?? []).filter((skill) => skill.seat === "task").map((skill) => skill.name),
    [skills.data],
  )
  const soleRepository = actualGoal?.repos.length === 1 ? actualGoal.repos[0] : undefined
  const taskItems = (tasks.data ?? [])
    .filter((task) => task.id !== editing?.id)
    .map((task) => ({ label: task.title, value: task.id }))
  const error = ApiError.is(submit.error) ? submit.error : null

  async function onSubmit(values: TaskFormValues) {
    try {
      const task = editing
        ? await update.mutateAsync(toUpdateTaskRequest(values))
        : await create.mutateAsync(toCreateTaskRequest(values))
      toast.success(editing ? "Task updated" : "Task created", { description: task.title })
      onOpenChange(false)
      if (!editing) onCreated?.(task)
    } catch {
      // Render the daemon error in the dialog.
    }
  }

  const goalPending = !goal && !editing?.execution_context && detail.isPending

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title={editing ? "Edit task" : "New task"}
        description="One workflow agent runs each column."
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          error
            ? {
                title: editing ? "Could not save task" : "Could not create task",
                error,
                showIcon: true,
              }
            : null
        }
        submitLabel={editing ? "Save changes" : "Create task"}
        pending={submit.isPending}
        submitDisabled={goalPending || !hasModel}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <Field data-invalid={form.formState.errors.title ? "" : undefined}>
            <FieldLabel htmlFor="task-title">Title</FieldLabel>
            <Input id="task-title" autoComplete="off" {...form.register("title")} />
            <FieldError>{form.formState.errors.title?.message}</FieldError>
          </Field>
          <Controller
            control={form.control}
            name="description"
            render={({ field }) => (
              <MarkdownField
                id="task-description"
                label="Description"
                description="Markdown for the workflow agents."
                value={field.value}
                onChange={field.onChange}
                onBlur={field.onBlur}
                name={field.name}
                ref={field.ref}
              />
            )}
          />
          <Field>
            <FieldLabel htmlFor="task-repo">Repository</FieldLabel>
            {goalPending || (!actualGoal && repositories.isPending) ? (
              <Skeleton aria-label="Loading repository" className="h-9 w-full" />
            ) : (
              <Controller
                control={form.control}
                name="repo_id"
                render={({ field }) => (
                  <RepositoryCombobox
                    id="task-repo"
                    repositories={actualGoal?.repos ?? repositories.data ?? []}
                    value={
                      editing
                        ? [editing.repo_id]
                        : field.value
                          ? [field.value]
                          : soleRepository
                            ? [soleRepository.id]
                            : []
                    }
                    onChange={(ids) => field.onChange(ids[0] ?? "")}
                    single
                    disabled={!!editing}
                  />
                )}
              />
            )}
          </Field>
          <Field>
            <FieldLabel>Workflow agents</FieldLabel>
            <div className="flex flex-col gap-3">
              {goalPending ? <Skeleton aria-label="Loading steps" className="h-20 w-full" /> : null}
              {steps.map((step, index) => (
                <div key={step.id} className="grid gap-2 sm:grid-cols-[10rem_1fr_1fr]">
                  <span className="pt-2 text-sm font-medium">{step.title}</span>
                  <SkillsInput
                    control={form.control}
                    name={`agents.${index}.skills`}
                    ariaLabel={`${step.title} skills`}
                    suggestions={skillNames}
                  />
                  <div>
                    <Controller
                      control={form.control}
                      name={`agents.${index}.model`}
                      render={({ field }) => (
                        <PinPicker
                          host={host}
                          label={`${step.title} runs on`}
                          model={field.value}
                          effort={agentValues[index]?.effort ?? ""}
                          onChange={(pin) => {
                            field.onChange(pin.model)
                            form.setValue(`agents.${index}.effort`, pin.effort, {
                              shouldDirty: true,
                            })
                          }}
                          models={models.data}
                        />
                      )}
                    />
                    {!agentValues[index]?.model.trim() ? (
                      <FieldDescription>Choose a model for {step.title}.</FieldDescription>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>
            <FieldDescription>Each row staffs one workflow column.</FieldDescription>
          </Field>
          {goalPending || tasks.isPending ? (
            <Skeleton aria-label="Loading dependencies" className="h-9 w-full" />
          ) : taskItems.length > 0 ? (
            <Field>
              <FieldLabel>Depends on</FieldLabel>
              {dependencies.fields.map((row, index) => (
                <div key={row.id} className="mb-2 flex gap-2">
                  <FormSelect
                    control={form.control}
                    name={`depends_on.${index}.task`}
                    options={taskItems}
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={`Remove dependency ${index + 1}`}
                    onClick={() => dependencies.remove(index)}
                  >
                    <XIcon />
                  </Button>
                </div>
              ))}
            </Field>
          ) : null}
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
