/**
 * Adopt one stored ACP conversation into a new task and either an active goal
 * or a new, unorchestrated goal.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { useLocation, useNavigate, useSearchParams } from "react-router-dom"
import { toast } from "sonner"
import { z } from "zod"

import { type AdoptOutsideSessionRequest, ApiError, type OutsideSessionDto } from "@/api"
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { goalsQueryOptions } from "@/features/goals/queries"
import { RepositoryCombobox } from "@/features/goals/repository-combobox"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { skillsQueryOptions } from "@/features/skills/queries"
import { SkillsInput } from "@/features/skills/skills-input"
import {
  makeTaskFormSchema,
  type TaskFormValues,
  taskToFormValues,
  toCreateTaskRequest,
} from "@/features/tasks/task-form-values"
import { LANDING_LABELS } from "@/lib/format"
import { taskPanelFrom } from "@/routes/paths"

import { useAdoptOutsideSession } from "./queries"

const LANDING_OPTIONS = (["merge", "pull_request", "none"] as const).map((value) => ({
  value,
  label: LANDING_LABELS[value],
}))

const PERMISSION_OPTIONS = [
  { value: "auto", label: "Automatic" },
  { value: "ask", label: "Ask every time" },
  { value: "learn", label: "Ask, then remember" },
]

const adoptionSchema = makeTaskFormSchema({ creating: true, requireRepo: false })
  .extend({
    goal_kind: z.enum(["existing", "new"]),
    goal_id: z.string(),
    goal_title: z.string(),
    goal_description: z.string(),
    repository_ids: z.array(z.string()),
    permission_mode: z.enum(["", "auto", "ask", "learn"]),
  })
  .superRefine((values, context) => {
    if (values.goal_kind === "existing" && values.goal_id.length === 0) {
      context.addIssue({
        code: "custom",
        path: ["goal_id"],
        message: "Choose an active goal.",
      })
    }
    if (values.goal_kind === "new" && values.goal_title.trim().length === 0) {
      context.addIssue({
        code: "custom",
        path: ["goal_title"],
        message: "Give the goal a title.",
      })
    }
  })

type AdoptionFormValues = z.infer<typeof adoptionSchema>

function defaults(session: OutsideSessionDto | null): AdoptionFormValues {
  return {
    ...taskToFormValues(undefined),
    title: session?.first_prompt ?? "",
    goal_kind: "existing",
    goal_id: "",
    goal_title: "",
    goal_description: "",
    repository_ids: [],
    permission_mode: "",
  }
}

/** The deepest registered checkout that contains the session directory. */
function containingRepository<T extends { id: string; path: string }>(
  workingDirectory: string,
  repositories: T[],
): T | undefined {
  return [...repositories]
    .filter(({ path }) => {
      const root = path.replace(/\/+$/, "") || "/"
      return root === "/" || workingDirectory === root || workingDirectory.startsWith(`${root}/`)
    })
    .sort((left, right) => right.path.length - left.path.length)[0]
}

export function AdoptOutsideSessionDialog({
  session,
  onClose,
}: {
  session: OutsideSessionDto | null
  onClose: () => void
}) {
  const open = session !== null
  const goals = useQuery({ ...goalsQueryOptions({ statuses: ["active"] }), enabled: open })
  const repositories = useQuery({ ...repositoriesQueryOptions(), enabled: open })
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
  const skills = useQuery({ ...skillsQueryOptions(), enabled: open })
  const adopt = useAdoptOutsideSession()
  const navigate = useNavigate()
  const location = useLocation()
  const [search] = useSearchParams()

  const defaultValues = useMemo(() => defaults(session), [session])
  const form = useForm<AdoptionFormValues>({
    resolver: zodResolver(adoptionSchema),
    defaultValues,
  })
  const reviewerRows = useFieldArray({ control: form.control, name: "reviewers" })
  useResetOnOpen(open, form, defaultValues, adopt)
  useClearErrorOnEdit(form, adopt)

  const activeGoals = useMemo(
    () => (goals.data ?? []).filter((goal) => goal.status === "active"),
    [goals.data],
  )
  const lockedModels = useMemo(
    () => (models.data ?? []).filter((model) => model.agent_id === session?.agent_id),
    [models.data, session?.agent_id],
  )
  const skillNames = useMemo(
    () => (skills.data ?? []).filter((skill) => skill.seat === "task").map((skill) => skill.name),
    [skills.data],
  )

  const goalKind = form.watch("goal_kind")
  const goalId = form.watch("goal_id")
  const repositoryIds = form.watch("repository_ids")
  const authorEffort = form.watch("author_effort")
  const reviewers = form.watch("reviewers")
  const selectedGoal = activeGoals.find((goal) => goal.id === goalId)
  const taskRepositories =
    goalKind === "existing"
      ? (selectedGoal?.repos ?? [])
      : (repositories.data ?? []).filter((repository) => repositoryIds.includes(repository.id))

  useEffect(() => {
    if (!open || !session || !repositories.data || form.formState.dirtyFields.repository_ids) return
    const match = containingRepository(session.working_directory, repositories.data)
    if (match && form.getValues("repository_ids").length === 0) {
      form.setValue("repository_ids", [match.id])
    }
  }, [form, open, repositories.data, session])

  useEffect(() => {
    const repoId = form.getValues("repo_id")
    if (repoId && !taskRepositories.some((repository) => repository.id === repoId)) {
      form.setValue("repo_id", "")
    }
  }, [form, taskRepositories])

  if (!session) return null

  const hasModels =
    form.watch("author_model").trim().length > 0 &&
    reviewers.every((reviewer) => reviewer.model.trim().length > 0)
  const hasGoal =
    goalKind === "existing" ? goalId.length > 0 : form.watch("goal_title").trim().length > 0

  async function onSubmit(values: AdoptionFormValues) {
    if (!session) return
    const task = toCreateTaskRequest(values as TaskFormValues, values.repo_id || null)
    const goal: AdoptOutsideSessionRequest["goal"] =
      values.goal_kind === "existing"
        ? { id: values.goal_id }
        : {
            title: values.goal_title.trim(),
            ...(values.goal_description.length > 0 ? { description: values.goal_description } : {}),
            ...(values.repository_ids.length > 0 ? { repository_ids: values.repository_ids } : {}),
          }
    const body: AdoptOutsideSessionRequest = {
      agent_id: session.agent_id,
      internal_session_id: session.internal_session_id,
      goal,
      title: task.title,
      description: task.description,
      agents: task.agents,
      landing: task.landing,
      ...(task.repo_id ? { repo_id: task.repo_id } : {}),
      ...(values.permission_mode ? { permission_mode: values.permission_mode } : {}),
    }

    try {
      const result = await adopt.mutateAsync(body)
      toast.success("Session adopted", { description: result.task.title })
      onClose()
      navigate(taskPanelFrom(location.pathname, search, result.task.id))
    } catch {
      // The daemon's message stays in the form.
    }
  }

  return (
    <FormDialog open onOpenChange={(next) => !next && onClose()} dirty={form.formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title="Adopt outside session"
        description={`Continue this ${session.agent_id} conversation as a task author.`}
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          ApiError.is(adopt.error)
            ? { title: "Could not adopt session", error: adopt.error, showIcon: true }
            : null
        }
        submitLabel="Adopt session"
        pending={adopt.isPending}
        submitDisabled={!hasGoal || !hasModels}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <section className="grid gap-3">
            <h3 className="font-heading text-base font-semibold">Goal</h3>
            <Tabs
              value={goalKind}
              onValueChange={(value) =>
                form.setValue("goal_kind", value as "existing" | "new", { shouldDirty: true })
              }
            >
              <TabsList>
                <TabsTrigger value="existing">Active goal</TabsTrigger>
                <TabsTrigger value="new">New goal</TabsTrigger>
              </TabsList>
              <TabsContent value="existing" className="mt-1">
                <Field data-invalid={form.formState.errors.goal_id ? "" : undefined}>
                  <FieldLabel htmlFor="adopt-active-goal">Active goal</FieldLabel>
                  <FormSelect
                    control={form.control}
                    name="goal_id"
                    id="adopt-active-goal"
                    options={activeGoals.map((goal) => ({ label: goal.title, value: goal.id }))}
                    placeholder="Choose an active goal"
                    invalid={form.formState.errors.goal_id ? true : undefined}
                  />
                  <FieldError>{form.formState.errors.goal_id?.message}</FieldError>
                </Field>
              </TabsContent>
              <TabsContent value="new" className="mt-1 grid gap-3">
                <Field data-invalid={form.formState.errors.goal_title ? "" : undefined}>
                  <FieldLabel htmlFor="adopt-goal-title">Goal title</FieldLabel>
                  <Input
                    id="adopt-goal-title"
                    autoComplete="off"
                    aria-invalid={form.formState.errors.goal_title ? true : undefined}
                    {...form.register("goal_title")}
                  />
                  <FieldError>{form.formState.errors.goal_title?.message}</FieldError>
                </Field>
                <Controller
                  control={form.control}
                  name="goal_description"
                  render={({ field }) => (
                    <MarkdownField
                      id="adopt-goal-description"
                      label="Goal description"
                      description="Markdown. This is the goal's brief."
                      value={field.value}
                      onChange={field.onChange}
                      onBlur={field.onBlur}
                      name={field.name}
                      ref={field.ref}
                    />
                  )}
                />
                <Controller
                  control={form.control}
                  name="repository_ids"
                  render={({ field }) => (
                    <Field>
                      <FieldLabel htmlFor="adopt-goal-repositories">Repositories</FieldLabel>
                      <RepositoryCombobox
                        id="adopt-goal-repositories"
                        repositories={repositories.data ?? []}
                        value={field.value}
                        onChange={field.onChange}
                      />
                      <FieldDescription>
                        The session directory selects its containing repository when available.
                      </FieldDescription>
                    </Field>
                  )}
                />
              </TabsContent>
            </Tabs>
          </section>

          <section className="grid gap-3 border-t pt-4">
            <h3 className="font-heading text-base font-semibold">Task</h3>
            <Field data-invalid={form.formState.errors.title ? "" : undefined}>
              <FieldLabel htmlFor="adopt-task-title">Task title</FieldLabel>
              <Input
                id="adopt-task-title"
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
                  id="adopt-task-description"
                  label="Task description"
                  description="Markdown. This is the author's brief."
                  value={field.value}
                  onChange={field.onChange}
                  onBlur={field.onBlur}
                  name={field.name}
                  ref={field.ref}
                />
              )}
            />
            <Field data-invalid={form.formState.errors.author_skills ? "" : undefined}>
              <FieldLabel htmlFor="adopt-author-skills">Author skills</FieldLabel>
              <SkillsInput
                control={form.control}
                name="author_skills"
                id="adopt-author-skills"
                suggestions={skillNames}
                invalid={form.formState.errors.author_skills ? true : undefined}
              />
              <FieldError>{form.formState.errors.author_skills?.message}</FieldError>
            </Field>
            <Field data-invalid={form.formState.errors.author_model ? "" : undefined}>
              <FieldLabel htmlFor="adopt-author-pin">Author runs on</FieldLabel>
              <Controller
                control={form.control}
                name="author_model"
                render={({ field }) => (
                  <PinPicker
                    id="adopt-author-pin"
                    label="Author runs on"
                    model={field.value}
                    effort={authorEffort}
                    onChange={(pin) => {
                      field.onChange(pin.model)
                      form.setValue("author_effort", pin.effort, { shouldDirty: true })
                    }}
                    models={lockedModels}
                    allowCustom={false}
                    invalid={form.formState.errors.author_model ? true : undefined}
                  />
                )}
              />
              <FieldDescription>The author stays on {session.agent_id}.</FieldDescription>
              <FieldError>{form.formState.errors.author_model?.message}</FieldError>
            </Field>

            <Field>
              <FieldLabel>Reviewers</FieldLabel>
              <div className="flex flex-col gap-2">
                {reviewerRows.fields.map((row, index) => {
                  const skillError = form.formState.errors.reviewers?.[index]?.skills
                  const modelError = form.formState.errors.reviewers?.[index]?.model
                  return (
                    <div key={row.id} className="flex flex-col gap-1">
                      <div className="flex items-start gap-2">
                        <SkillsInput
                          control={form.control}
                          name={`reviewers.${index}.skills`}
                          ariaLabel={`Reviewer ${index + 1} skills`}
                          suggestions={skillNames}
                          invalid={skillError ? true : undefined}
                          className="flex-1"
                        />
                        <Controller
                          control={form.control}
                          name={`reviewers.${index}.model`}
                          render={({ field }) => (
                            <PinPicker
                              label={`Reviewer ${index + 1} runs on`}
                              model={field.value}
                              effort={reviewers[index]?.effort ?? ""}
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
                      <FieldError>{skillError?.message ?? modelError?.message}</FieldError>
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
            </Field>

            {taskRepositories.length > 1 ? (
              <Field>
                <FieldLabel htmlFor="adopt-task-repository">Repository</FieldLabel>
                <FormSelect
                  control={form.control}
                  name="repo_id"
                  id="adopt-task-repository"
                  options={taskRepositories.map((repository) => ({
                    label: repository.path,
                    value: repository.id,
                  }))}
                  placeholder="Infer from working directory"
                  renderOption={(repository) => (
                    <span className="font-mono text-xs">{repository.label}</span>
                  )}
                />
              </Field>
            ) : null}

            <Field>
              <FieldLabel htmlFor="adopt-task-landing">Ends with</FieldLabel>
              <FormSelect
                control={form.control}
                name="landing"
                id="adopt-task-landing"
                options={LANDING_OPTIONS}
                empty="merge"
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="adopt-task-permission-mode">Permission mode</FieldLabel>
              <FormSelect
                control={form.control}
                name="permission_mode"
                id="adopt-task-permission-mode"
                options={PERMISSION_OPTIONS}
                placeholder="Use daemon default"
              />
            </Field>
          </section>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
