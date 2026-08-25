/**
 * The task form: `ariadne task create` in the goal panel, and `ariadne task
 * update` in the task panel, as one dialog with two modes.
 *
 * The daemon does the real validation — profile roles, repo membership, dep
 * cycles, `max_tasks`, and for edits the pending/ready guard — so the client
 * only catches what it can know on its own (empty title, no reviewer, a
 * reviewer picked twice) and shows the daemon's error envelope verbatim for
 * everything else, with the dialog staying open. That covers the stale edit:
 * a task that started while the form was open answers `409` right here.
 *
 * Reviewers are an ordered list, not a set: the daemon spawns them in the
 * order given, so the field is rows that keep their order rather than a
 * multi-select. Dependencies get the same rows for the same look, though
 * their order carries no meaning. On edit both replace the task's lists.
 *
 * The engineer and the repo can only be chosen at creation — `PATCH
 * /v1/tasks/{id}` carries neither — so edit mode leaves those fields out; the
 * task panel's facts card keeps showing what they are.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { ApiError, type GoalDto, type TaskDto } from "@/api"
import {
  FormDialog,
  FormDialogContent,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect, profilePlaceholder } from "@/components/form-select"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { profilesQueryOptions } from "@/features/profiles/queries"
import { taskListQueryOptions, useCreateTask, useUpdateTask } from "./queries"
import {
  makeTaskFormSchema,
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
  toUpdateTaskRequest,
} from "./task-form-values"

export function CreateTaskDialog({
  goal,
  open,
  onOpenChange,
  onCreated,
}: {
  goal: GoalDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (task: TaskDto) => void
}) {
  return (
    <TaskFormDialog goal={goal} open={open} onOpenChange={onOpenChange} onCreated={onCreated} />
  )
}

export function EditTaskDialog({
  task,
  open,
  onOpenChange,
}: {
  task: TaskDto
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return <TaskFormDialog editing={task} open={open} onOpenChange={onOpenChange} />
}

function TaskFormDialog({
  goal,
  editing,
  open,
  onOpenChange,
  onCreated,
}: {
  /** Create mode: the goal the task goes into. */
  goal?: GoalDto
  /** Edit mode: the pending/ready task whose fields the form starts from. */
  editing?: TaskDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (task: TaskDto) => void
}) {
  const goalId = goal?.id ?? editing?.goal_id ?? ""
  const engineers = useQuery({ ...profilesQueryOptions("engineer"), enabled: open && !editing })
  const reviewers = useQuery({ ...profilesQueryOptions("reviewer"), enabled: open })
  // The dependency choices are the goal's own tasks — the same query the Tasks
  // tab behind this dialog holds, so it is usually already cached.
  const goalTasks = useQuery({ ...taskListQueryOptions({ goal: goalId }), enabled: open })
  const createTask = useCreateTask(goalId)
  const updateTask = useUpdateTask(editing?.id ?? "")
  const submit = editing ? updateTask : createTask

  const multiRepo = (goal?.repos.length ?? 0) > 1
  const formSchema = useMemo(
    () => makeTaskFormSchema({ creating: !editing, requireRepo: multiRepo }),
    [editing, multiRepo],
  )
  const defaultValues = useMemo(() => taskToFormValues(editing), [editing])

  const form = useForm<TaskFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  const dependsRows = useFieldArray({ control: form.control, name: "depends_on" })

  useResetOnOpen(open, form, defaultValues, submit)

  const engineerOptions = useMemo(
    () => [...(engineers.data ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [engineers.data],
  )
  const reviewerOptions = useMemo(
    () => [...(reviewers.data ?? [])].sort((a, b) => a.name.localeCompare(b.name)),
    [reviewers.data],
  )
  const engineerItems = useMemo(
    () => engineerOptions.map((profile) => ({ label: profile.name, value: profile.id })),
    [engineerOptions],
  )
  const reviewerItems = useMemo(
    () => reviewerOptions.map((profile) => ({ label: profile.name, value: profile.id })),
    [reviewerOptions],
  )
  const repoItems = useMemo(
    () => (goal?.repos ?? []).map((repo) => ({ label: repo.path, value: repo.id })),
    [goal?.repos],
  )
  // A task cannot depend on itself, so editing drops it from its own choices.
  const dependencyChoices = useMemo(
    () => (goalTasks.data ?? []).filter((task) => task.id !== editing?.id),
    [goalTasks.data, editing?.id],
  )
  const taskItems = useMemo(
    () => dependencyChoices.map((task) => ({ label: task.title, value: task.id })),
    [dependencyChoices],
  )

  // The daemon ships built-in "Engineer" and "Reviewer" profiles; preselect
  // them (or the only choice there is) so the common case is one click, the
  // same way the goal form preselects its planner. Create only: an edited task
  // already has both.
  const selectedEngineer = form.watch("engineer_profile")
  useEffect(() => {
    if (!open || editing || !engineerOptions.length || selectedEngineer) return
    const preferred =
      engineerOptions.find((profile) => profile.name === "Engineer") ?? engineerOptions[0]
    if (preferred) form.setValue("engineer_profile", preferred.id)
  }, [open, editing, engineerOptions, selectedEngineer, form.setValue])

  const firstReviewer = form.watch("reviewers.0.profile")
  useEffect(() => {
    if (!open || editing || !reviewerOptions.length || firstReviewer !== "") return
    const preferred =
      reviewerOptions.find((profile) => profile.name === "Reviewer") ?? reviewerOptions[0]
    if (preferred) form.setValue("reviewers.0.profile", preferred.id)
  }, [open, editing, reviewerOptions, firstReviewer, form.setValue])

  const submitError = ApiError.is(submit.error) ? submit.error : null
  useClearErrorOnEdit(form, submit)

  async function onSubmit(values: TaskFormValues) {
    try {
      if (editing) {
        const task = await updateTask.mutateAsync(toUpdateTaskRequest(values))
        toast.success("Task updated", { description: task.title })
        onOpenChange(false)
      } else {
        const task = await createTask.mutateAsync(
          toCreateTaskRequest(values, multiRepo ? values.repo_id : null),
        )
        toast.success("Task created", { description: task.title })
        onOpenChange(false)
        onCreated?.(task)
      }
    } catch {
      // Rendered inline by the dialog: the daemon's message is the useful part.
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
        title={editing ? "Edit task" : "New task"}
        description={
          editing
            ? "Editable while the task is still waiting; reviewers and dependencies replace the current lists. The engineer and the repository are fixed at creation."
            : "A unit of work an engineer takes from branch to merge, reviewed by the reviewers in the order given."
        }
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          submitError
            ? {
                title: editing ? "Could not save task" : "Could not create task",
                error: submitError,
                showIcon: true,
              }
            : null
        }
        submitLabel={editing ? "Save changes" : "Create task"}
        pending={submit.isPending}
      >
        <Field data-invalid={form.formState.errors.title ? "" : undefined}>
          <FieldLabel htmlFor="task-title">Title</FieldLabel>
          <Input
            id="task-title"
            autoComplete="off"
            aria-invalid={form.formState.errors.title ? true : undefined}
            {...form.register("title")}
          />
          <FieldError>{form.formState.errors.title?.message}</FieldError>
        </Field>

        <Field>
          <FieldLabel htmlFor="task-description">Description</FieldLabel>
          <Textarea
            id="task-description"
            placeholder="What should be built, and anything the engineer needs to know."
            {...form.register("description")}
          />
          <FieldDescription>Markdown. This is the engineer's brief.</FieldDescription>
        </Field>

        {!editing ? (
          <Field data-invalid={form.formState.errors.engineer_profile ? "" : undefined}>
            <FieldLabel htmlFor="task-engineer">Engineer profile</FieldLabel>
            <FormSelect
              control={form.control}
              name="engineer_profile"
              id="task-engineer"
              options={engineerItems}
              disabled={!engineerOptions.length}
              placeholder={profilePlaceholder(engineers, "engineer")}
            />
            <FieldError>{form.formState.errors.engineer_profile?.message}</FieldError>
          </Field>
        ) : null}

        <Field>
          <FieldLabel>Reviewer profiles</FieldLabel>
          <div className="flex flex-col gap-2">
            {reviewerRows.fields.map((row, index) => {
              const error = form.formState.errors.reviewers?.[index]?.profile
              return (
                <div key={row.id} className="flex flex-col gap-1">
                  <div className="flex items-start gap-2">
                    <FormSelect
                      control={form.control}
                      name={`reviewers.${index}.profile`}
                      ariaLabel={`Reviewer ${index + 1}`}
                      invalid={error ? true : undefined}
                      className="flex-1"
                      options={reviewerItems}
                      disabled={!reviewerOptions.length}
                      placeholder={profilePlaceholder(reviewers, "reviewer")}
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={`Remove reviewer ${index + 1}`}
                      disabled={reviewerRows.fields.length === 1}
                      onClick={() => reviewerRows.remove(index)}
                    >
                      <XIcon />
                    </Button>
                  </div>
                  <FieldError>{error?.message}</FieldError>
                </div>
              )
            })}
          </div>
          <div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => reviewerRows.append({ profile: "" })}
            >
              <PlusIcon />
              Add reviewer
            </Button>
          </div>
          <FieldDescription>
            The task is reviewed by each of these, top to bottom, every round.
          </FieldDescription>
          <FieldError>{form.formState.errors.reviewers?.root?.message}</FieldError>
        </Field>

        {multiRepo ? (
          <Field data-invalid={form.formState.errors.repo_id ? "" : undefined}>
            <FieldLabel htmlFor="task-repo">Repository</FieldLabel>
            <FormSelect
              control={form.control}
              name="repo_id"
              id="task-repo"
              options={repoItems}
              placeholder="Select a repository"
              renderOption={(repo) => <span className="font-mono text-xs">{repo.label}</span>}
            />
            <FieldDescription>The repo the task's branch and worktree live in.</FieldDescription>
            <FieldError>{form.formState.errors.repo_id?.message}</FieldError>
          </Field>
        ) : null}

        {taskItems.length > 0 ? (
          <Field>
            <FieldLabel>Depends on</FieldLabel>
            {dependsRows.fields.length > 0 ? (
              <div className="flex flex-col gap-2">
                {dependsRows.fields.map((row, index) => (
                  <div key={row.id} className="flex items-start gap-2">
                    <FormSelect
                      control={form.control}
                      name={`depends_on.${index}.task`}
                      ariaLabel={`Dependency ${index + 1}`}
                      className="flex-1"
                      options={taskItems}
                      placeholder="Select a task"
                      renderOption={(task) => (
                        <span className="flex min-w-0 items-baseline gap-2">
                          <span className="truncate">{task.label}</span>
                          <span className="shrink-0 font-mono text-xs text-muted-foreground">
                            {task.value}
                          </span>
                        </span>
                      )}
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={`Remove dependency ${index + 1}`}
                      onClick={() => dependsRows.remove(index)}
                    >
                      <XIcon />
                    </Button>
                  </div>
                ))}
              </div>
            ) : null}
            <div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => dependsRows.append({ task: "" })}
              >
                <PlusIcon />
                Add dependency
              </Button>
            </div>
            <FieldDescription>
              Tasks that must merge before this one starts. Blank rows are dropped.
            </FieldDescription>
          </Field>
        ) : null}
      </FormDialogContent>
    </FormDialog>
  )
}
