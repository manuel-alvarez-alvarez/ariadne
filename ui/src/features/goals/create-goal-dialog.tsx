/**
 * `ariadne goal create`, as a form.
 *
 * Repositories are picked, not typed: a goal is created against checkouts that
 * are already registered (`/repositories`), which is where a new one is added
 * — the checks that make a path a repository belong on the screen that owns
 * them, not on every form that needs one. With none registered the field is an
 * empty state pointing there.
 *
 * What the planner runs on is one field: a model, written
 * `<agent_kind>[:<model>]` — the agent CLI and, after a `:`, the model of it.
 * An empty box carries a meaning of its own, the planner on its profile's own
 * model, so it is left out of the request rather than sent empty.
 *
 * Everything else the daemon still validates: the client only catches what it
 * can know on its own (empty title, nothing picked) and shows the daemon's
 * error envelope verbatim.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { useEffect, useMemo } from "react"
import { Controller, useForm } from "react-hook-form"
import { Link } from "react-router-dom"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type CreateGoalRequest, type GoalDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect, profilePlaceholder } from "@/components/form-select"
import { MarkdownField } from "@/components/markdown-field"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { ModelPicker } from "@/features/profiles/model-picker"
import { modelRefField, modelRefLabel } from "@/features/profiles/model-ref"
import { modelsQueryOptions } from "@/features/profiles/queries"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { paths } from "@/routes/paths"

import { plannerProfilesQueryOptions, useCreateGoal } from "./queries"
import { RepositoryCombobox } from "./repository-combobox"

/** Optional positive integer, kept as the string the input holds. */
function optionalCount(label: string) {
  return z
    .string()
    .trim()
    .refine((value) => value === "" || /^[1-9]\d*$/.test(value), {
      message: `${label} must be a positive whole number.`,
    })
}

const formSchema = z.object({
  title: z.string().trim().min(1, "Give the goal a title."),
  description: z.string(),
  planner_profile: z.string().min(1, "Choose a planner profile."),
  // Free text: the catalog only suggests, and a model it does not carry is
  // handed to the CLI named before the `:` as typed.
  model: modelRefField(),
  required_approvals: optionalCount("Approvals"),
  max_tasks: optionalCount("Max tasks"),
  repository_ids: z.array(z.string()).min(1, "Pick at least one repository."),
})

type CreateGoalForm = z.infer<typeof formSchema>

const DEFAULT_VALUES: CreateGoalForm = {
  title: "",
  description: "",
  planner_profile: "",
  model: "",
  required_approvals: "1",
  max_tasks: "",
  repository_ids: [],
}

export function CreateGoalDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (goal: GoalDto) => void
}) {
  const planners = useQuery({ ...plannerProfilesQueryOptions(), enabled: open })
  const repositories = useQuery({ ...repositoriesQueryOptions(), enabled: open })
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
  const createGoal = useCreateGoal()

  const form = useForm<CreateGoalForm>({
    resolver: zodResolver(formSchema),
    defaultValues: DEFAULT_VALUES,
  })
  const errors = form.formState.errors

  useResetOnOpen(open, form, DEFAULT_VALUES, createGoal)
  useClearErrorOnEdit(form, createGoal)

  // The daemon's own default is the built-in "Planner" profile; match it so the
  // common case is one click.
  const plannerOptions = planners.data
  const plannerItems = useMemo(
    () => (plannerOptions ?? []).map((profile) => ({ label: profile.name, value: profile.id })),
    [plannerOptions],
  )
  const selectedPlanner = form.watch("planner_profile")
  const plannerProfile = plannerOptions?.find((profile) => profile.id === selectedPlanner)
  useEffect(() => {
    if (!open || !plannerOptions?.length || selectedPlanner) return
    const preferred =
      plannerOptions.find((profile) => profile.name === "Planner") ?? plannerOptions[0]
    if (preferred) form.setValue("planner_profile", preferred.id)
  }, [open, plannerOptions, selectedPlanner, form.setValue])

  async function onSubmit(values: CreateGoalForm) {
    const model = values.model.trim()
    const body: CreateGoalRequest = {
      title: values.title.trim(),
      description: values.description,
      planner_profile: values.planner_profile,
      // No field at all where the box was left empty: that is the planner on
      // its profile's own model.
      ...(model.length > 0 ? { model } : {}),
      repository_ids: values.repository_ids,
      required_approvals: values.required_approvals ? Number(values.required_approvals) : null,
      max_tasks: values.max_tasks ? Number(values.max_tasks) : null,
    }
    try {
      const goal = await createGoal.mutateAsync(body)
      toast.success("Goal created", { description: goal.title })
      onOpenChange(false)
      onCreated?.(goal)
    } catch {
      // Rendered inline by the dialog: the daemon's message is the useful part.
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title="New goal"
        description="The planner reads the description, then proposes the tasks in the goal thread."
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          ApiError.is(createGoal.error)
            ? { title: "Could not create goal", error: createGoal.error, showIcon: true }
            : null
        }
        submitLabel="Create goal"
        pending={createGoal.isPending}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <Field data-invalid={errors.title ? "" : undefined}>
            <FieldLabel htmlFor="goal-title">Title</FieldLabel>
            <Input
              id="goal-title"
              autoComplete="off"
              aria-invalid={errors.title ? true : undefined}
              {...form.register("title")}
            />
            <FieldError>{errors.title?.message}</FieldError>
          </Field>

          <Controller
            control={form.control}
            name="description"
            render={({ field }) => (
              <MarkdownField
                id="goal-description"
                label="Description"
                description="Markdown. This is the planner's brief."
                placeholder="What should be achieved, and anything the planner needs to know."
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
              <Field data-invalid={errors.repository_ids ? "" : undefined}>
                {/* One control to point a `for` at, now that the row of
                    checkboxes is a single combobox: the label names the
                    trigger, and the picked set is spelled out in it. */}
                <FieldLabel htmlFor="goal-repositories">Repositories</FieldLabel>
                {repositories.isPending ? (
                  <LoadingRepositories />
                ) : repositories.isError ? (
                  <ErrorState
                    title="Could not load repositories"
                    error={repositories.error}
                    onRetry={() => void repositories.refetch()}
                  />
                ) : repositories.data.length === 0 ? (
                  <NoRepositories onLeave={() => onOpenChange(false)} />
                ) : (
                  <RepositoryCombobox
                    id="goal-repositories"
                    repositories={repositories.data}
                    value={field.value}
                    onChange={field.onChange}
                    invalid={errors.repository_ids ? true : undefined}
                  />
                )}
                <FieldDescription>
                  The checkouts the planner splits this goal across. Task worktrees are branched off
                  each one's base branch.
                </FieldDescription>
                <FieldError>{errors.repository_ids?.message}</FieldError>
              </Field>
            )}
          />

          <div className="grid gap-4 sm:grid-cols-3">
            <Field data-invalid={errors.planner_profile ? "" : undefined}>
              <FieldLabel htmlFor="goal-planner">Planner profile</FieldLabel>
              <FormSelect
                control={form.control}
                name="planner_profile"
                id="goal-planner"
                options={plannerItems}
                disabled={!plannerOptions?.length}
                placeholder={profilePlaceholder(planners, "planner")}
              />
              <FieldError>{errors.planner_profile?.message}</FieldError>
            </Field>

            <Field data-invalid={errors.required_approvals ? "" : undefined}>
              <FieldLabel htmlFor="goal-approvals">Approvals</FieldLabel>
              <Input
                id="goal-approvals"
                inputMode="numeric"
                autoComplete="off"
                aria-invalid={errors.required_approvals ? true : undefined}
                {...form.register("required_approvals")}
              />
              <FieldError>{errors.required_approvals?.message}</FieldError>
            </Field>

            <Field data-invalid={errors.max_tasks ? "" : undefined}>
              <FieldLabel htmlFor="goal-max-tasks">Max tasks</FieldLabel>
              <Input
                id="goal-max-tasks"
                inputMode="numeric"
                placeholder="unbounded"
                autoComplete="off"
                aria-invalid={errors.max_tasks ? true : undefined}
                {...form.register("max_tasks")}
              />
              <FieldError>{errors.max_tasks?.message}</FieldError>
            </Field>
          </div>

          <Field data-invalid={errors.model ? "" : undefined}>
            {/* No htmlFor: cmdk owns its input's id and names it itself. */}
            <FieldLabel>Planner model</FieldLabel>
            <Controller
              control={form.control}
              name="model"
              render={({ field }) => (
                <ModelPicker
                  label="Planner model"
                  value={field.value}
                  onChange={field.onChange}
                  models={models.data}
                  invalid={errors.model ? true : undefined}
                  placeholder={
                    plannerProfile
                      ? `The profile's own — ${modelRefLabel(plannerProfile.model)}`
                      : "The profile's own"
                  }
                />
              )}
            />
            {errors.model ? (
              <FieldError>{errors.model.message}</FieldError>
            ) : (
              <FieldDescription>
                The agent CLI and, after a <code>:</code>, the model of it. Empty runs the planner
                on its profile's own: {modelRefLabel(plannerProfile?.model)}.
              </FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}

/** Standing in for a list whose length is not known until it arrives. */
function LoadingRepositories() {
  return (
    <div className="flex flex-col gap-2 rounded-lg border p-3">
      <Skeleton className="h-4 w-2/3" />
      <Skeleton className="h-4 w-1/2" />
    </div>
  )
}

/**
 * Nothing to pick, and the way out of that.
 *
 * Select-only on purpose: registering a checkout is its own screen's job —
 * the daemon opens it, resolves the branch and checks it has commits — so the
 * dialog links there rather than growing a second form inside itself.
 */
function NoRepositories({ onLeave }: { onLeave: () => void }) {
  return (
    <EmptyState
      emphasis="quiet"
      title="No repositories registered"
      description="A goal is created against registered checkouts. Register one first, then come back."
      action={
        <Button
          variant="outline"
          size="sm"
          // The dialog is modal, so it has to come down with the navigation or
          // it would sit over the screen it just sent the user to.
          onClick={onLeave}
          render={<Link to={paths.repositories()} />}
        >
          Go to Repositories
        </Button>
      }
    />
  )
}
