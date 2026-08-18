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
 * The engineer profile and the repo can only be chosen at creation — `PATCH
 * /v1/tasks/{id}` does not carry them — so edit mode leaves both fields out;
 * the task panel's facts card keeps showing what they are.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo, useRef } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import {
  ApiError,
  type CreateTaskRequest,
  type GoalDto,
  type TaskDto,
  type UpdateTaskRequest,
} from "@/api"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { profilesQueryOptions } from "@/features/profiles/queries"
import { taskListQueryOptions, useCreateTask, useUpdateTask } from "./queries"

/**
 * One schema for both modes. The repo is required only when the goal has more
 * than one (with a single repo the daemon infers it and the field is not even
 * shown), and the engineer only on create — edit mode neither shows nor sends
 * either field.
 */
function makeFormSchema(opts: { requireEngineer: boolean; requireRepo: boolean }) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: opts.requireEngineer
      ? z.string().min(1, "Choose an engineer profile.")
      : z.string(),
    reviewers: z
      .array(z.object({ profile: z.string().min(1, "Choose a reviewer profile.") }))
      .min(1, "A task needs at least one reviewer.")
      .superRefine((rows, ctx) => {
        const seen = new Set<string>()
        rows.forEach((row, index) => {
          if (!row.profile) return
          if (seen.has(row.profile)) {
            ctx.addIssue({
              code: z.ZodIssueCode.custom,
              path: [index, "profile"],
              message: "This reviewer is already in the list.",
            })
          }
          seen.add(row.profile)
        })
      }),
    repo_id: opts.requireRepo ? z.string().min(1, "Choose a repository.") : z.string(),
    // Blank rows are dropped on submit, like the profile form's flags.
    depends_on: z.array(z.object({ task: z.string() })),
  })
}

type TaskFormValues = z.infer<ReturnType<typeof makeFormSchema>>

const CREATE_DEFAULTS: TaskFormValues = {
  title: "",
  description: "",
  engineer_profile: "",
  reviewers: [{ profile: "" }],
  repo_id: "",
  depends_on: [],
}

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
    () => makeFormSchema({ requireEngineer: !editing, requireRepo: multiRepo }),
    [editing, multiRepo],
  )

  const defaultValues = useMemo<TaskFormValues>(
    () =>
      editing
        ? {
            title: editing.title,
            description: editing.description,
            engineer_profile: editing.engineer_profile_id,
            reviewers: editing.reviewer_profile_ids.map((profile) => ({ profile })),
            repo_id: "",
            depends_on: editing.depends_on.map((task) => ({ task })),
          }
        : CREATE_DEFAULTS,
    [editing],
  )

  const form = useForm<TaskFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  const dependsRows = useFieldArray({ control: form.control, name: "depends_on" })

  // Re-opening starts from a clean form — the task as it stands, or blank —
  // never from the previous attempt. The defaults go through a ref so only
  // opening resets: a `task_updated` off the stream mid-edit must not wipe
  // what the user has typed.
  const defaultValuesRef = useRef(defaultValues)
  defaultValuesRef.current = defaultValues
  useEffect(() => {
    if (open) {
      form.reset(defaultValuesRef.current)
      submit.reset()
    }
  }, [open, form.reset, submit.reset])

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
  // same way the goal form preselects its planner. Create only: an edited
  // task already has both.
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

  // "goal already has 3 of max 3 tasks" (or "task is in_progress") must not
  // still be on screen once the form has changed, so the first edit after a
  // failure drops the alert.
  useEffect(() => {
    if (!submitError) return
    const subscription = form.watch(() => submit.reset())
    return () => subscription.unsubscribe()
  }, [submitError, form.watch, submit.reset])

  async function onSubmit(values: TaskFormValues) {
    const dependsOn = [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
    try {
      if (editing) {
        const body: UpdateTaskRequest = {
          title: values.title.trim(),
          description: values.description,
          reviewer_profiles: values.reviewers.map((row) => row.profile),
          depends_on: dependsOn,
        }
        const task = await updateTask.mutateAsync(body)
        toast.success("Task updated", { description: task.title })
        onOpenChange(false)
      } else {
        const body: CreateTaskRequest = {
          title: values.title.trim(),
          description: values.description,
          engineer_profile: values.engineer_profile,
          reviewer_profiles: values.reviewers.map((row) => row.profile),
          repo_id: multiRepo ? values.repo_id : null,
          depends_on: dependsOn,
        }
        const task = await createTask.mutateAsync(body)
        toast.success("Task created", { description: task.title })
        onOpenChange(false)
        onCreated?.(task)
      }
    } catch {
      // Rendered inline below: the daemon's message is the useful part.
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit task" : "New task"}</DialogTitle>
            <DialogDescription>
              {editing ? (
                <>
                  Editable while the task is still waiting; reviewers and dependencies replace the
                  current lists. The engineer profile and the repository are fixed at creation.
                </>
              ) : (
                <>
                  A unit of work an engineer takes from branch to merge, reviewed by the reviewers
                  in the order given.
                </>
              )}
            </DialogDescription>
          </DialogHeader>

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
              rows={5}
              placeholder="What should be built, and anything the engineer needs to know."
              {...form.register("description")}
            />
            <FieldDescription>Markdown. This is the engineer's brief.</FieldDescription>
          </Field>

          {!editing ? (
            <Field data-invalid={form.formState.errors.engineer_profile ? "" : undefined}>
              <FieldLabel htmlFor="task-engineer">Engineer profile</FieldLabel>
              <Controller
                control={form.control}
                name="engineer_profile"
                render={({ field }) => (
                  <Select
                    value={field.value || null}
                    onValueChange={(value) => field.onChange(value ?? "")}
                    disabled={!engineerOptions.length}
                    // Without this the trigger would show the raw profile id.
                    items={engineerItems}
                  >
                    <SelectTrigger id="task-engineer" className="w-full">
                      <SelectValue placeholder={profilePlaceholder(engineers, "engineer")} />
                    </SelectTrigger>
                    <SelectContent>
                      {engineerOptions.map((profile) => (
                        <SelectItem key={profile.id} value={profile.id}>
                          {profile.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
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
                      <Controller
                        control={form.control}
                        name={`reviewers.${index}.profile`}
                        render={({ field }) => (
                          <Select
                            value={field.value || null}
                            onValueChange={(value) => field.onChange(value ?? "")}
                            disabled={!reviewerOptions.length}
                            items={reviewerItems}
                          >
                            <SelectTrigger
                              aria-label={`Reviewer ${index + 1}`}
                              aria-invalid={error ? true : undefined}
                              className="w-full flex-1"
                            >
                              <SelectValue
                                placeholder={profilePlaceholder(reviewers, "reviewer")}
                              />
                            </SelectTrigger>
                            <SelectContent>
                              {reviewerOptions.map((profile) => (
                                <SelectItem key={profile.id} value={profile.id}>
                                  {profile.name}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
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
              <Controller
                control={form.control}
                name="repo_id"
                render={({ field }) => (
                  <Select
                    value={field.value || null}
                    onValueChange={(value) => field.onChange(value ?? "")}
                    items={repoItems}
                  >
                    <SelectTrigger id="task-repo" className="w-full">
                      <SelectValue placeholder="Select a repository" />
                    </SelectTrigger>
                    <SelectContent>
                      {(goal?.repos ?? []).map((repo) => (
                        <SelectItem key={repo.id} value={repo.id}>
                          <span className="font-mono text-xs">{repo.path}</span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
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
                      <Controller
                        control={form.control}
                        name={`depends_on.${index}.task`}
                        render={({ field }) => (
                          <Select
                            value={field.value || null}
                            onValueChange={(value) => field.onChange(value ?? "")}
                            items={taskItems}
                          >
                            <SelectTrigger
                              aria-label={`Dependency ${index + 1}`}
                              className="w-full flex-1"
                            >
                              <SelectValue placeholder="Select a task" />
                            </SelectTrigger>
                            <SelectContent>
                              {dependencyChoices.map((task) => (
                                <SelectItem key={task.id} value={task.id}>
                                  <span className="flex min-w-0 items-baseline gap-2">
                                    <span className="truncate">{task.title}</span>
                                    <span className="shrink-0 font-mono text-xs text-muted-foreground">
                                      {task.id}
                                    </span>
                                  </span>
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
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

          {submitError ? (
            <ErrorState
              showIcon
              title={editing ? "Could not save task" : "Could not create task"}
              error={submitError}
            />
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" pending={submit.isPending}>
              {editing ? "Save changes" : "Create task"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function profilePlaceholder(
  profiles: { isPending: boolean; isError: boolean; data?: unknown[] },
  role: string,
): string {
  if (profiles.isPending) return "Loading…"
  if (profiles.isError) return "Profiles unavailable"
  if (!profiles.data?.length) return `No ${role} profiles`
  return "Select a profile"
}
