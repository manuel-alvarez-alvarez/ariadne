/**
 * `ariadne task create`, as a form in the goal panel.
 *
 * The daemon does the real validation — profile roles, repo membership, dep
 * cycles, `max_tasks` — so the client only catches what it can know on its own
 * (empty title, no reviewer, a reviewer picked twice) and shows the daemon's
 * error envelope verbatim for everything else, with the dialog staying open.
 *
 * Reviewers are an ordered list, not a set: the daemon spawns them in the
 * order given, so the field is rows that keep their order rather than a
 * multi-select. Dependencies get the same rows for the same look, though
 * their order carries no meaning.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type CreateTaskRequest, type GoalDto, type TaskDto } from "@/api"
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
import { taskListQueryOptions, useCreateTask } from "./queries"

/**
 * The repo is the one field whose being required depends on the goal: with a
 * single repo the daemon infers it and the field is not even shown.
 */
function makeFormSchema(requireRepo: boolean) {
  return z.object({
    title: z.string().trim().min(1, "Give the task a title."),
    description: z.string(),
    engineer_profile: z.string().min(1, "Choose an engineer profile."),
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
    repo_id: requireRepo ? z.string().min(1, "Choose a repository.") : z.string(),
    // Blank rows are dropped on submit, like the profile form's flags.
    depends_on: z.array(z.object({ task: z.string() })),
  })
}

type CreateTaskForm = z.infer<ReturnType<typeof makeFormSchema>>

const DEFAULT_VALUES: CreateTaskForm = {
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
  const engineers = useQuery({ ...profilesQueryOptions("engineer"), enabled: open })
  const reviewers = useQuery({ ...profilesQueryOptions("reviewer"), enabled: open })
  // The dependency choices are the goal's own tasks — the same query the Tasks
  // tab behind this dialog holds, so it is usually already cached.
  const goalTasks = useQuery({ ...taskListQueryOptions({ goal: goal.id }), enabled: open })
  const createTask = useCreateTask(goal.id)

  const multiRepo = goal.repos.length > 1
  const formSchema = useMemo(() => makeFormSchema(multiRepo), [multiRepo])

  const form = useForm<CreateTaskForm>({
    resolver: zodResolver(formSchema),
    defaultValues: DEFAULT_VALUES,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  const dependsRows = useFieldArray({ control: form.control, name: "depends_on" })

  // Re-opening starts from a clean form, never from the previous attempt.
  useEffect(() => {
    if (open) {
      form.reset(DEFAULT_VALUES)
      createTask.reset()
    }
  }, [open, form.reset, createTask.reset])

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
    () => goal.repos.map((repo) => ({ label: repo.path, value: repo.id })),
    [goal.repos],
  )
  const taskItems = useMemo(
    () => (goalTasks.data ?? []).map((task) => ({ label: task.title, value: task.id })),
    [goalTasks.data],
  )

  // The daemon ships built-in "Engineer" and "Reviewer" profiles; preselect
  // them (or the only choice there is) so the common case is one click, the
  // same way the goal form preselects its planner.
  const selectedEngineer = form.watch("engineer_profile")
  useEffect(() => {
    if (!open || !engineerOptions.length || selectedEngineer) return
    const preferred =
      engineerOptions.find((profile) => profile.name === "Engineer") ?? engineerOptions[0]
    if (preferred) form.setValue("engineer_profile", preferred.id)
  }, [open, engineerOptions, selectedEngineer, form.setValue])

  const firstReviewer = form.watch("reviewers.0.profile")
  useEffect(() => {
    if (!open || !reviewerOptions.length || firstReviewer !== "") return
    const preferred =
      reviewerOptions.find((profile) => profile.name === "Reviewer") ?? reviewerOptions[0]
    if (preferred) form.setValue("reviewers.0.profile", preferred.id)
  }, [open, reviewerOptions, firstReviewer, form.setValue])

  const submitError = ApiError.is(createTask.error) ? createTask.error : null

  // "goal already has 3 of max 3 tasks" must not still be on screen once the
  // form has changed, so the first edit after a failure drops the alert.
  useEffect(() => {
    if (!submitError) return
    const subscription = form.watch(() => createTask.reset())
    return () => subscription.unsubscribe()
  }, [submitError, form.watch, createTask.reset])

  async function onSubmit(values: CreateTaskForm) {
    const dependsOn = [...new Set(values.depends_on.map((row) => row.task).filter(Boolean))]
    const body: CreateTaskRequest = {
      title: values.title.trim(),
      description: values.description,
      engineer_profile: values.engineer_profile,
      reviewer_profiles: values.reviewers.map((row) => row.profile),
      repo_id: multiRepo ? values.repo_id : null,
      depends_on: dependsOn,
    }
    try {
      const task = await createTask.mutateAsync(body)
      toast.success("Task created", { description: task.title })
      onOpenChange(false)
      onCreated?.(task)
    } catch {
      // Rendered inline below: the daemon's message is the useful part.
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>New task</DialogTitle>
            <DialogDescription>
              A unit of work an engineer takes from branch to merge, reviewed by the reviewers in
              the order given.
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
                      {goal.repos.map((repo) => (
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
                              {(goalTasks.data ?? []).map((task) => (
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
            <ErrorState showIcon title="Could not create task" error={submitError} />
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" pending={createTask.isPending}>
              Create task
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
