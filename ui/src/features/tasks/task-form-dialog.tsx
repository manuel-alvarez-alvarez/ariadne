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
 *
 * The models can be chosen in both modes, one per agent: the engineer's and
 * every reviewer's. A model is the whole of that choice — the daemon derives
 * the agent CLI from it — so there is no agent control here, and an empty box
 * means the profile's own model. A model the daemon cannot place comes back as
 * a `400` naming it, which is shown under the box that holds it rather than
 * over the whole form: it is one field's problem, and the other fields are
 * still worth keeping.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { ApiError, type GoalDto, type ProfileDto, type TaskDto } from "@/api"
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
import { ModelPicker } from "@/features/profiles/model-picker"
import { modelsQueryOptions, profilesQueryOptions } from "@/features/profiles/queries"
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
  // Read in both modes now: edit mode does not offer the engineer, but the
  // engineer's profile is what its model box is read and placeheld against.
  const engineers = useQuery({ ...profilesQueryOptions("engineer"), enabled: open })
  const reviewers = useQuery({ ...profilesQueryOptions("reviewer"), enabled: open })
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
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
  // The profiles are what a pin is read against: a model box means "run this
  // on something else", so a pin that only repeats the profile's own model
  // opens empty. They usually come from the cache the panel behind this dialog
  // already filled; the effect below covers the cold open.
  const knownProfiles = useMemo(
    () => [...(engineers.data ?? []), ...(reviewers.data ?? [])],
    [engineers.data, reviewers.data],
  )
  const defaultValues = useMemo(
    () => taskToFormValues(editing, knownProfiles),
    [editing, knownProfiles],
  )

  const form = useForm<TaskFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  const dependsRows = useFieldArray({ control: form.control, name: "depends_on" })

  useResetOnOpen(open, form, defaultValues, submit)

  // Re-seed once the profiles arrive after the dialog opened, which is the
  // only thing that can change what the form should have started from. Never
  // over anything typed: the moment a field is touched the form is the user's.
  const { isDirty } = form.formState
  useEffect(() => {
    if (open && !isDirty) form.reset(defaultValues)
  }, [open, isDirty, defaultValues, form.reset])

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

  // The engineer the form is on: the picked one while creating, the task's own
  // while editing. It is what the engineer's model box is placeheld against.
  const selectedEngineer = form.watch("engineer_profile")
  const engineerProfile = useMemo(
    () => engineerOptions.find((profile) => profile.id === selectedEngineer),
    [engineerOptions, selectedEngineer],
  )
  // Watched whole, so each row's box knows the profile it sits beside.
  const reviewerRowValues = form.watch("reviewers")

  // The daemon ships built-in "Engineer" and "Reviewer" profiles; preselect
  // them (or the only choice there is) so the common case is one click, the
  // same way the goal form preselects its planner. Create only: an edited task
  // already has both.
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

  // A model the daemon could not place is one box's problem: it names the id
  // it refused, so the message goes under whichever box holds it and the form
  // keeps the rest. Derived rather than pushed into the form's errors, so the
  // first edit clears it along with the mutation (`useClearErrorOnEdit`).
  const refused = refusedModel(submitError)
  const modelError = (value: string) =>
    refused !== null && value.trim() === refused ? submitError?.message : undefined

  async function onSubmit(values: TaskFormValues) {
    try {
      if (editing) {
        // The baseline is what the form was last *reset* with, which react-hook-form
        // keeps for us — not `defaultValues`, which goes on changing as the
        // profiles arrive even where the form was left alone on purpose (see
        // the re-seed effect above, and `toUpdateTaskRequest`).
        const seeded = form.formState.defaultValues?.engineer_model ?? ""
        const task = await updateTask.mutateAsync(toUpdateTaskRequest(values, seeded))
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
            ? "Editable while the task is still waiting; reviewers and dependencies replace the current lists. The engineer's profile and the repository are fixed at creation — the model it runs on is not."
            : "A unit of work an engineer takes from branch to merge, reviewed by the reviewers in the order given."
        }
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          // Not when a model field is carrying it: one message, under the box
          // it is about.
          submitError && refused === null
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

        {/* Editable in both modes, unlike the profile beside it: the daemon
            takes a model on `PATCH` too, for as long as the task waits. */}
        <Field data-invalid={modelError(form.watch("engineer_model")) ? "" : undefined}>
          {/* No htmlFor: cmdk owns its input's id and names it itself. */}
          <FieldLabel>Engineer model</FieldLabel>
          <Controller
            control={form.control}
            name="engineer_model"
            render={({ field }) => (
              <ModelPicker
                label="Engineer model"
                value={field.value}
                onChange={field.onChange}
                models={models.data}
                placeholder={modelPlaceholder(engineerProfile)}
                caption
                invalid={modelError(field.value) ? true : undefined}
              />
            )}
          />
          <FieldDescription>
            The agent CLI follows the model. Leave it empty to run on the profile's own.
          </FieldDescription>
          <FieldError>{modelError(form.watch("engineer_model"))}</FieldError>
        </Field>

        <Field>
          {/* A row is a reviewer now, not only a profile: the slot carries the
              model that reviewer runs on. */}
          <FieldLabel>Reviewers</FieldLabel>
          <div className="flex flex-col gap-2">
            {reviewerRows.fields.map((row, index) => {
              const error = form.formState.errors.reviewers?.[index]?.profile
              const chosen = reviewerRowValues?.[index]
              const profile = reviewerOptions.find((option) => option.id === chosen?.profile)
              const rejected = modelError(chosen?.model ?? "")
              return (
                <div key={row.id} className="flex flex-col gap-1">
                  {/* The profile and what it runs on, side by side: one row is
                      one reviewer, and its model belongs to the slot rather
                      than to the profile it names. */}
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
                    <Controller
                      control={form.control}
                      name={`reviewers.${index}.model`}
                      render={({ field }) => (
                        <ModelPicker
                          label={`Reviewer ${index + 1} model`}
                          value={field.value}
                          onChange={field.onChange}
                          models={models.data}
                          placeholder={modelPlaceholder(profile)}
                          caption
                          invalid={rejected ? true : undefined}
                          className="flex-1"
                        />
                      )}
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
                  <FieldError>{error?.message ?? rejected}</FieldError>
                </div>
              )
            })}
          </div>
          <div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => reviewerRows.append({ profile: "", model: "" })}
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

/**
 * What an empty model box would run on: the profile's own model, named where
 * it has one so that leaving the box alone is an informed choice rather than a
 * blank.
 */
function modelPlaceholder(profile: ProfileDto | undefined): string {
  return profile?.model ? `Profile default: ${profile.model}` : "Profile default: the CLI's default"
}

/**
 * The model id a `400` refused, or null when the failure was about anything
 * else. The daemon answers a model it cannot place by naming it — "unknown
 * model `llama3`: cannot tell which agent runs it" — which is what lets the
 * message be shown under the box that holds it.
 */
function refusedModel(error: ApiError | null): string | null {
  if (error?.status !== 400) return null
  return error.message.match(/unknown model `([^`]+)`/)?.[1] ?? null
}
