/**
 * The task form: `ariadne task create` in the goal panel, and `ariadne task
 * update` in the task panel, as one dialog with two modes.
 *
 * The daemon does the real validation — the skills a name answers to, repo
 * membership, dep cycles, `max_tasks`, and for edits the pending/ready guard
 * — so the client only catches what it can know on its own (an empty title,
 * an agent with no skill) and shows the daemon's error envelope verbatim for
 * everything else, with the dialog staying open. That covers the stale edit:
 * a task that started while the form was open answers `409` right here.
 *
 * Reviewers are an ordered list, not a set: the daemon spawns them in the
 * order given, so the field is rows that keep their order rather than a
 * multi-select. Dependencies get the same rows for the same look, though
 * their order carries no meaning. On edit both replace the task's lists.
 *
 * The author's skills and the repo can only be chosen at creation — `PATCH
 * /v1/tasks/{id}` carries neither — so edit mode leaves those fields out; the
 * task panel's facts card keeps showing what they are.
 *
 * What each agent runs on can be chosen in both modes, one control per agent:
 * the author's and every reviewer's. The pin is a model — the agent CLI and,
 * after a `:`, the model of it — and the effort that model is run at, and one
 * picker holds both, so a reviewer row stays three controls wide: the skills,
 * what they run on, and the remove. Nothing pinned is auto, which the picker
 * is told so it can say so.
 *
 * The reviewers can be none: most work is worth a second pair of eyes and the
 * form starts with one, but a task with nothing to review — a release, say —
 * is approved as soon as its author asks.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { ApiError, type GoalDto, type Landing, type TaskDto } from "@/api"
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
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { skillsQueryOptions } from "@/features/skills/queries"
import { SkillsInput } from "@/features/skills/skills-input"
import { LANDING_LABELS } from "@/lib/format"
import { taskListQueryOptions, useCreateTask, useUpdateTask } from "./queries"
import {
  makeTaskFormSchema,
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
  toUpdateTaskRequest,
} from "./task-form-values"

/**
 * How a task can end, in the order a reader meets them: the ordinary one, the
 * one that hands the change to a person, and the one that lands nothing.
 */
const LANDING_OPTIONS = (["merge", "pull_request", "none"] as const satisfies Landing[]).map(
  (value) => ({ value, label: LANDING_LABELS[value] }),
)

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
  // The catalog behind the skill boxes: a suggestion, not a gate — the daemon
  // is what refuses a name no skill answers to.
  const skills = useQuery({ ...skillsQueryOptions(), enabled: open })
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
  // Nothing async goes into the seeding any more: an agent's pin is simply
  // its pin, so what the form starts from is the task and nothing else.
  const defaultValues = useMemo(() => taskToFormValues(editing), [editing])

  const form = useForm<TaskFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  const dependsRows = useFieldArray({ control: form.control, name: "depends_on" })

  useResetOnOpen(open, form, defaultValues, submit)

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

  // The effort each agent is pinned at, which its picker holds beside the
  // model: one control, two fields.
  const authorEffort = form.watch("author_effort")
  const reviewerRowValues = form.watch("reviewers")
  /** Every skill name the daemon knows, as the boxes suggest them. */
  const skillNames = useMemo(() => (skills.data ?? []).map((skill) => skill.name), [skills.data])

  const submitError = ApiError.is(submit.error) ? submit.error : null
  useClearErrorOnEdit(form, submit)

  async function onSubmit(values: TaskFormValues) {
    try {
      if (editing) {
        // The baseline is what the form was last *reset* with, which react-hook-form
        // keeps for us — not `defaultValues`, which goes on changing as the
        // profiles arrive even where the form was left alone on purpose (see
        // the re-seed effect above, and `toUpdateTaskRequest`).
        const seeded = form.formState.defaultValues
        const task = await updateTask.mutateAsync(
          toUpdateTaskRequest(values, {
            model: seeded?.author_model ?? "",
            effort: seeded?.author_effort ?? "",
          }),
        )
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
        className="sm:max-w-2xl"
        title={editing ? "Edit task" : "New task"}
        description={
          editing
            ? "Editable while the task is still waiting; reviewers and dependencies replace the current lists. The author's skills and the repository are fixed at creation — the model it runs on is not."
            : "A unit of work an author takes from branch to merge, reviewed by the reviewers in the order given."
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
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
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

          <Controller
            control={form.control}
            name="description"
            render={({ field }) => (
              <MarkdownField
                id="task-description"
                label="Description"
                description="Markdown. This is the author's brief."
                placeholder="What should be built, and anything the author needs to know."
                value={field.value}
                onChange={field.onChange}
                onBlur={field.onBlur}
                name={field.name}
                ref={field.ref}
              />
            )}
          />

          {!editing ? (
            <Field data-invalid={form.formState.errors.author_skills ? "" : undefined}>
              <FieldLabel htmlFor="task-author-skills">Author skills</FieldLabel>
              <SkillsInput
                control={form.control}
                name="author_skills"
                id="task-author-skills"
                suggestions={skillNames}
              />
              {form.formState.errors.author_skills ? (
                <FieldError>{form.formState.errors.author_skills.message}</FieldError>
              ) : (
                <FieldDescription>
                  Comma-separated, in the order they reach the agent. They are the whole of what it
                  can do.
                </FieldDescription>
              )}
            </Field>
          ) : null}

          {/* Editable in both modes, unlike the skills beside it: a task keeps
              the author it started with, but the daemon takes a pin on `PATCH`
              too, for as long as the task waits. */}
          <Field data-invalid={form.formState.errors.author_model ? "" : undefined}>
            <FieldLabel htmlFor="task-author-pin">Author runs on</FieldLabel>
            <Controller
              control={form.control}
              name="author_model"
              render={({ field }) => (
                <PinPicker
                  id="task-author-pin"
                  label="Author runs on"
                  model={field.value}
                  effort={authorEffort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    form.setValue("author_effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models.data}
                  invalid={form.formState.errors.author_model ? true : undefined}
                  unpinnedLabel="auto — first installed CLI, on its own default model"
                />
              )}
            />
            {form.formState.errors.author_model ? (
              <FieldError>{form.formState.errors.author_model.message}</FieldError>
            ) : (
              <FieldDescription>
                The agent CLI and, after a <code>:</code>, the model of it. Empty is auto.
              </FieldDescription>
            )}
          </Field>

          <Field>
            {/* A row is one reviewer: the skills it reviews with, and what it
                runs on. */}
            <FieldLabel>Reviewers</FieldLabel>
            <div className="flex flex-col gap-2">
              {reviewerRows.fields.map((row, index) => {
                const error = form.formState.errors.reviewers?.[index]?.skills
                const modelError = form.formState.errors.reviewers?.[index]?.model
                return (
                  <div key={row.id} className="flex flex-col gap-1">
                    {/* The skills and what they run on, side by side: one row
                        is one reviewer, and both belong to that agent. */}
                    <div className="flex items-start gap-2">
                      <SkillsInput
                        control={form.control}
                        name={`reviewers.${index}.skills`}
                        ariaLabel={`Reviewer ${index + 1} skills`}
                        invalid={error ? true : undefined}
                        className="flex-1"
                        suggestions={skillNames}
                      />
                      <Controller
                        control={form.control}
                        name={`reviewers.${index}.model`}
                        render={({ field }) => (
                          <PinPicker
                            label={`Reviewer ${index + 1} runs on`}
                            model={field.value}
                            effort={reviewerRowValues?.[index]?.effort ?? ""}
                            onChange={(pin) => {
                              field.onChange(pin.model)
                              form.setValue(`reviewers.${index}.effort`, pin.effort, {
                                shouldDirty: true,
                              })
                            }}
                            models={models.data}
                            invalid={modelError ? true : undefined}
                            className="flex-1"
                          />
                        )}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label={`Remove reviewer ${index + 1}`}
                        onClick={() => reviewerRows.remove(index)}
                      >
                        <XIcon />
                      </Button>
                    </div>
                    <FieldError>{error?.message ?? modelError?.message}</FieldError>
                  </div>
                )
              })}
            </div>
            <div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => reviewerRows.append({ skills: "", model: "", effort: "" })}
              >
                <PlusIcon />
                Add reviewer
              </Button>
            </div>
            <FieldDescription>
              The task is reviewed by each of these, top to bottom, every round. Leave it empty only
              where there is nothing to review: the task is then approved as soon as its author
              asks.
            </FieldDescription>
            <FieldError>{form.formState.errors.reviewers?.root?.message}</FieldError>
          </Field>

          <Field>
            <FieldLabel htmlFor="task-landing">Ends with</FieldLabel>
            <FormSelect
              control={form.control}
              name="landing"
              id="task-landing"
              options={LANDING_OPTIONS}
              empty="merge"
            />
            <FieldDescription>
              What happens to the work when the task is approved. Most tasks put the change on the
              base branch; some leave a request for a person, and some land nothing at all — a
              release, a report, a document that lives elsewhere.
            </FieldDescription>
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
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
