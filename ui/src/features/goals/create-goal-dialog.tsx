/**
 * `ariadne goal create`, as a form.
 *
 * Repositories are picked, not typed: a goal is created against checkouts that
 * are already registered (`/repositories`), which is where a new one is added
 * — the checks that make a path a repository belong on the screen that owns
 * them, not on every form that needs one. With none registered the field is an
 * empty state pointing there.
 *
 * What the orchestrator runs on is one choice made in one control: a model, written
 * `<agent_kind>:<model>` — the agent CLI and, after a `:`, the model of it —
 * and the effort that model is run at. A model is required and the empty effort
 * uses the model's default effort.
 *
 * Everything else the daemon still validates: the client only catches what it
 * can know on its own (empty title, nothing picked) and shows the daemon's
 * error envelope verbatim.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { Controller, useForm } from "react-hook-form"
import { Link } from "react-router-dom"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type CreateGoalRequest, type GoalDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { MarkdownField } from "@/components/markdown-field"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { modelRefField } from "@/features/models/model-ref"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { NoRepositories as SharedNoRepositories } from "@/features/repositories/no-repositories"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { paths } from "@/routes/paths"

import { useCreateGoal } from "./queries"
import { RepositoryCombobox } from "./repository-combobox"

const formSchema = z.object({
  title: z.string().trim().min(1, "Give the goal a title."),
  description: z.string(),
  // Free text: the catalog only suggests, and a model it does not carry is
  // handed to the CLI named before the `:` as typed.
  model: modelRefField(),
  // The effort that model is run at, scoped by the box beside it; empty is
  // whatever the agent CLI runs it at.
  effort: z.string(),
  repository_ids: z.array(z.string()).min(1, "Pick at least one repository."),
})

type CreateGoalForm = z.infer<typeof formSchema>

const DEFAULT_VALUES: CreateGoalForm = {
  title: "",
  description: "",
  model: "",
  effort: "",
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

  // The effort the orchestrator is pinned at, which the picker holds beside
  // the model: one control, two fields.
  const chosenEffort = form.watch("effort")

  async function onSubmit(values: CreateGoalForm) {
    const model = values.model.trim()
    const effort = values.effort.trim()
    const body: CreateGoalRequest = {
      title: values.title.trim(),
      description: values.description,
      model,
      ...(effort.length > 0 ? { effort } : {}),
      repository_ids: values.repository_ids,
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
        description="The orchestrator reads the description, then proposes the tasks in the goal thread."
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          ApiError.is(createGoal.error)
            ? { title: "Could not create goal", error: createGoal.error, showIcon: true }
            : null
        }
        submitLabel="Create goal"
        pending={createGoal.isPending}
        submitDisabled={form.watch("model").trim().length === 0}
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
                description="Markdown. This is the orchestrator's brief."
                placeholder="What should be achieved, and anything the orchestrator needs to know."
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
                  The checkouts the orchestrator splits this goal across. Task worktrees are
                  branched off each one's base branch.
                </FieldDescription>
                <FieldError>{errors.repository_ids?.message}</FieldError>
              </Field>
            )}
          />

          <Field data-invalid={errors.model ? "" : undefined}>
            <FieldLabel htmlFor="goal-pin">Orchestrator runs on</FieldLabel>
            <Controller
              control={form.control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="goal-pin"
                  label="Orchestrator runs on"
                  model={field.value}
                  effort={chosenEffort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    form.setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models.data}
                  invalid={errors.model ? true : undefined}
                />
              )}
            />
            {errors.model ? (
              <FieldError>{errors.model.message}</FieldError>
            ) : (
              <FieldDescription>
                The agent CLI and, after a <code>:</code>, the model of it.
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
 * dialog links there rather than growing a second form inside itself. What the
 * state is *called* is shared with that screen (`no-repositories.tsx`); only
 * the way out is this dialog's own.
 */
function NoRepositories({ onLeave }: { onLeave: () => void }) {
  return (
    <SharedNoRepositories
      emphasis="quiet"
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
