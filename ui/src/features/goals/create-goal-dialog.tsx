/**
 * `ariadne goal create`, as a form.
 *
 * The daemon does the real validation — it opens every repo, resolves the base
 * branch and checks it has commits — so the client only catches what it can
 * know on its own (empty title, relative path) and shows the daemon's error
 * envelope verbatim for everything else.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect, useMemo } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type CreateGoalRequest, type GoalDto } from "@/api"
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
  repos: z
    .array(
      z.object({
        path: z
          .string()
          .trim()
          .min(1, "Enter the repository path.")
          .refine((value) => value.startsWith("/"), {
            message: "The path must be absolute.",
          }),
        base_branch: z.string().trim(),
      }),
    )
    .min(1, "A goal needs at least one repository."),
})

type CreateGoalForm = z.infer<typeof formSchema>

const EMPTY_REPO = { path: "", base_branch: "" }

const DEFAULT_VALUES: CreateGoalForm = {
  title: "",
  description: "",
  planner_profile: "",
  required_approvals: "1",
  max_tasks: "",
  repos: [EMPTY_REPO],
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
  const createGoal = useCreateGoal()

  const form = useForm<CreateGoalForm>({
    resolver: zodResolver(formSchema),
    defaultValues: DEFAULT_VALUES,
  })
  const repos = useFieldArray({ control: form.control, name: "repos" })

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
      repos: values.repos.map((repo) => ({
        path: repo.path.trim(),
        base_branch: repo.base_branch.trim() || null,
      })),
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

          <Field>
            <FieldLabel>Repositories</FieldLabel>
            <div className="flex flex-col gap-2">
              {repos.fields.map((field, index) => {
                const errors = form.formState.errors.repos?.[index]
                return (
                  <div key={field.id} className="flex flex-col gap-1">
                    <div className="flex items-start gap-2">
                      <Input
                        aria-label={`Repository ${index + 1} path`}
                        placeholder="/absolute/path/to/repo"
                        spellCheck={false}
                        autoComplete="off"
                        className="flex-2"
                        aria-invalid={errors?.path ? true : undefined}
                        {...form.register(`repos.${index}.path`)}
                      />
                      <Input
                        aria-label={`Repository ${index + 1} base branch`}
                        placeholder="base branch (optional)"
                        spellCheck={false}
                        autoComplete="off"
                        className="flex-1"
                        {...form.register(`repos.${index}.base_branch`)}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label={`Remove repository ${index + 1}`}
                        disabled={repos.fields.length === 1}
                        onClick={() => repos.remove(index)}
                      >
                        <XIcon />
                      </Button>
                    </div>
                    <FieldError>{errors?.path?.message}</FieldError>
                  </div>
                )
              })}
            </div>
            <div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => repos.append(EMPTY_REPO)}
              >
                <PlusIcon />
                Add repository
              </Button>
            </div>
            <FieldDescription>
              Absolute paths to existing git repositories. An empty base branch means the repo's
              current branch.
            </FieldDescription>
            <FieldError>{form.formState.errors.repos?.root?.message}</FieldError>
          </Field>

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
            <ErrorState title="Could not create the goal" error={submitError} />
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={createGoal.isPending}>
              {createGoal.isPending ? "Creating…" : "Create goal"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
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
