/**
 * `ariadne goal create`, as a form.
 *
 * Repositories are picked, not typed: a goal is created against checkouts that
 * are already registered (`/repositories`), which is where a new one is added
 * — the checks that make a path a repository belong on the screen that owns
 * them, not on every form that needs one. With none registered the field is an
 * empty state pointing there.
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
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldLabel, FieldTitle } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Textarea } from "@/components/ui/textarea"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { paths } from "@/routes/paths"

import { plannerProfilesQueryOptions, useCreateGoal } from "./queries"

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
  required_approvals: optionalCount("Approvals"),
  max_tasks: optionalCount("Max tasks"),
  repository_ids: z.array(z.string()).min(1, "Pick at least one repository."),
})

type CreateGoalForm = z.infer<typeof formSchema>

const DEFAULT_VALUES: CreateGoalForm = {
  title: "",
  description: "",
  planner_profile: "",
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
  const createGoal = useCreateGoal()

  const form = useForm<CreateGoalForm>({
    resolver: zodResolver(formSchema),
    defaultValues: DEFAULT_VALUES,
  })

  // Re-opening starts from a clean form, never from the previous attempt.
  useEffect(() => {
    if (open) {
      form.reset(DEFAULT_VALUES)
      createGoal.reset()
    }
  }, [open, form.reset, createGoal.reset])

  // The daemon's own default is the built-in "Planner" profile; match it so the
  // common case is one click.
  const plannerOptions = planners.data
  const plannerItems = useMemo(
    () => (plannerOptions ?? []).map((profile) => ({ label: profile.name, value: profile.id })),
    [plannerOptions],
  )
  const selectedPlanner = form.watch("planner_profile")
  useEffect(() => {
    if (!open || !plannerOptions?.length || selectedPlanner) return
    const preferred =
      plannerOptions.find((profile) => profile.name === "Planner") ?? plannerOptions[0]
    if (preferred) form.setValue("planner_profile", preferred.id)
  }, [open, plannerOptions, selectedPlanner, form.setValue])

  const submitError = ApiError.is(createGoal.error) ? createGoal.error : null

  // "repo path does not exist" must not still be on screen once the path has
  // been corrected, so the first edit after a failure drops the alert.
  useEffect(() => {
    if (!submitError) return
    const subscription = form.watch(() => createGoal.reset())
    return () => subscription.unsubscribe()
  }, [submitError, form.watch, createGoal.reset])

  async function onSubmit(values: CreateGoalForm) {
    const body: CreateGoalRequest = {
      title: values.title.trim(),
      description: values.description,
      planner_profile: values.planner_profile,
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
      // Rendered inline below: the daemon's message is the useful part.
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>New goal</DialogTitle>
            <DialogDescription>
              The planner reads the description, then proposes the tasks in the goal thread.
            </DialogDescription>
          </DialogHeader>

          <Field data-invalid={form.formState.errors.title ? "" : undefined}>
            <FieldLabel htmlFor="goal-title">Title</FieldLabel>
            <Input
              id="goal-title"
              autoComplete="off"
              aria-invalid={form.formState.errors.title ? true : undefined}
              {...form.register("title")}
            />
            <FieldError>{form.formState.errors.title?.message}</FieldError>
          </Field>

          <Field>
            <FieldLabel htmlFor="goal-description">Description</FieldLabel>
            <Textarea
              id="goal-description"
              rows={5}
              placeholder="What should be achieved, and anything the planner needs to know."
              {...form.register("description")}
            />
            <FieldDescription>Markdown. This is the planner's brief.</FieldDescription>
          </Field>

          <Controller
            control={form.control}
            name="repository_ids"
            render={({ field }) => (
              <Field
                aria-label="Repositories"
                data-invalid={form.formState.errors.repository_ids ? "" : undefined}
              >
                {/* A heading rather than a label: what follows is a list of
                    checkboxes, not one control to point a `for` at. Each row
                    carries its own label instead. */}
                <FieldTitle>Repositories</FieldTitle>
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
                  <div className="flex max-h-56 w-full flex-col gap-0.5 overflow-y-auto rounded-lg border p-1">
                    {repositories.data.map((repository) => {
                      // Base UI's checkbox puts this on the hidden input it
                      // renders beside the box, which is what the row's label
                      // points at; the box itself is named by `aria-label`.
                      const inputId = `goal-repository-${repository.id}`
                      return (
                        <label
                          key={repository.id}
                          htmlFor={inputId}
                          className="flex cursor-pointer items-start gap-2.5 rounded-md px-2 py-1.5 hover:bg-accent/60"
                        >
                          <Checkbox
                            id={inputId}
                            className="mt-0.5"
                            aria-label={repository.path}
                            checked={field.value.includes(repository.id)}
                            onCheckedChange={(checked) =>
                              field.onChange(
                                checked
                                  ? [...field.value, repository.id]
                                  : field.value.filter((id) => id !== repository.id),
                              )
                            }
                          />
                          <span className="min-w-0 flex-1 font-normal">
                            <span className="flex flex-wrap items-baseline gap-x-2">
                              <span className="font-mono text-xs">{repository.path}</span>
                              <span className="font-mono text-xs text-muted-foreground">
                                {repository.base_branch}
                              </span>
                            </span>
                            {repository.description ? (
                              <span className="block text-xs text-muted-foreground">
                                {repository.description}
                              </span>
                            ) : null}
                          </span>
                        </label>
                      )
                    })}
                  </div>
                )}
                <FieldDescription>
                  The checkouts the planner splits this goal across. Task worktrees are branched off
                  each one's base branch.
                </FieldDescription>
                <FieldError>{form.formState.errors.repository_ids?.message}</FieldError>
              </Field>
            )}
          />

          <div className="grid gap-4 sm:grid-cols-3">
            <Field data-invalid={form.formState.errors.planner_profile ? "" : undefined}>
              <FieldLabel htmlFor="goal-planner">Planner profile</FieldLabel>
              <Controller
                control={form.control}
                name="planner_profile"
                render={({ field }) => (
                  <Select
                    value={field.value || null}
                    onValueChange={(value) => field.onChange(value ?? "")}
                    disabled={!plannerOptions?.length}
                    // Without this the trigger would show the raw profile id.
                    items={plannerItems}
                  >
                    <SelectTrigger id="goal-planner" className="w-full">
                      <SelectValue placeholder={plannerPlaceholder(planners)} />
                    </SelectTrigger>
                    <SelectContent>
                      {plannerOptions?.map((profile) => (
                        <SelectItem key={profile.id} value={profile.id}>
                          {profile.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              />
              <FieldError>{form.formState.errors.planner_profile?.message}</FieldError>
            </Field>

            <Field data-invalid={form.formState.errors.required_approvals ? "" : undefined}>
              <FieldLabel htmlFor="goal-approvals">Approvals</FieldLabel>
              <Input
                id="goal-approvals"
                inputMode="numeric"
                autoComplete="off"
                aria-invalid={form.formState.errors.required_approvals ? true : undefined}
                {...form.register("required_approvals")}
              />
              <FieldError>{form.formState.errors.required_approvals?.message}</FieldError>
            </Field>

            <Field data-invalid={form.formState.errors.max_tasks ? "" : undefined}>
              <FieldLabel htmlFor="goal-max-tasks">Max tasks</FieldLabel>
              <Input
                id="goal-max-tasks"
                inputMode="numeric"
                placeholder="unbounded"
                autoComplete="off"
                aria-invalid={form.formState.errors.max_tasks ? true : undefined}
                {...form.register("max_tasks")}
              />
              <FieldError>{form.formState.errors.max_tasks?.message}</FieldError>
            </Field>
          </div>

          {submitError ? (
            <ErrorState showIcon title="Could not create goal" error={submitError} />
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" pending={createGoal.isPending}>
              Create goal
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
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

function plannerPlaceholder(planners: {
  isPending: boolean
  isError: boolean
  data?: unknown[]
}): string {
  if (planners.isPending) return "Loading…"
  if (planners.isError) return "Profiles unavailable"
  if (!planners.data?.length) return "No planner profiles"
  return "Select a profile"
}
