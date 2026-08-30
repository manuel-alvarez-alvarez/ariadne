/**
 * Create and edit dialog for a repository — one form for both, because the two
 * differ only in where they post and in what an omitted base branch means.
 *
 * The client only catches what it can know on its own: a missing or relative
 * path. Everything else is the daemon's to say — it opens the checkout,
 * resolves the branch and checks it has commits, and rejects a landing
 * briefing that names a placeholder it has no value for — so a 400 lands on
 * the field it is about (the path, the branch when one was typed, or the
 * briefing) and a 409 says the pair is already registered.
 *
 * The landing briefing is prefilled from `GET /v1/merge-strategies` — the
 * selected strategy's built-in text — for a new repository, or from the
 * row's own `landing_prompt` for an existing one; either way it is *that*
 * default the daemon is asked to keep sending "the default", not a copy of
 * today's text, which is why a default briefing goes to the daemon as absent
 * (create) or empty (edit) rather than as the words themselves.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { RotateCcwIcon } from "lucide-react"
import { useEffect, useMemo, useRef } from "react"
import { useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type MergeStrategy, type RepositoryDto } from "@/api"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { describeError } from "@/lib/format"

import { mergeStrategiesQueryOptions, useCreateRepository, useUpdateRepository } from "./queries"

const formSchema = z.object({
  path: z
    .string()
    .trim()
    .min(1, "Enter the repository path.")
    .refine((value) => value.startsWith("/"), { message: "The path must be absolute." }),
  base_branch: z.string().trim(),
  description: z.string(),
  merge_strategy: z.enum(["direct", "pull_request"]),
  landing_prompt: z.string(),
})

type RepositoryFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: RepositoryFormValues = {
  path: "",
  base_branch: "",
  description: "",
  merge_strategy: "direct",
  landing_prompt: "",
}

/**
 * How an approved task reaches the base branch: the name of each strategy, and
 * what it means for the engineer that has to act on it.
 *
 * The name is the short one, and it is the *only* one — the repositories table
 * shows the same word for the same stored value, which it could not do while
 * this form called `direct` "Squash onto the base branch" and the table called
 * it "Direct". The sentence is the option's description rather than its name,
 * beside it in the list and under the field once one is picked.
 */
export const MERGE_STRATEGY_META: Record<MergeStrategy, { label: string; description: string }> = {
  direct: {
    label: "Direct",
    description:
      "The engineer rebases, squashes the task into one commit and fast-forwards the base branch itself.",
  },
  pull_request: {
    label: "Pull request",
    description:
      "The engineer opens a request with `gh` or `glab`, answers what is written on it, and finishes the task once it is merged.",
  },
}

/** The strategies as a select's options, in the order they are offered. */
const MERGE_STRATEGIES = (Object.keys(MERGE_STRATEGY_META) as MergeStrategy[]).map((value) => ({
  value,
  label: MERGE_STRATEGY_META[value].label,
}))

export function RepositoryFormDialog({
  open,
  onOpenChange,
  repository,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The repository being edited, or null to register a new one. */
  repository: RepositoryDto | null
}) {
  const editing = repository !== null
  const createRepository = useCreateRepository()
  const updateRepository = useUpdateRepository()
  const saving = createRepository.isPending || updateRepository.isPending

  // Static data — the strategies and their built-in text never change once
  // the daemon is up — read once and reused for the prefill, the swap on a
  // strategy change, and the reset button. `enabled: open`, like every other
  // query this dialog only needs while it is up: the dialog stays mounted
  // between opens, so an unguarded query would fetch on every page load.
  const strategies = useQuery({ ...mergeStrategiesQueryOptions(), enabled: open })
  const defaultsByStrategy = useMemo(() => {
    const map: Partial<Record<MergeStrategy, string>> = {}
    for (const entry of strategies.data ?? []) map[entry.merge_strategy] = entry.landing_prompt
    return map
  }, [strategies.data])

  const form = useForm<RepositoryFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { formState, getValues, handleSubmit, register, reset, setError, setValue, watch } = form
  const selectedStrategy = watch("merge_strategy")
  const landingPrompt = watch("landing_prompt")
  const selectedDefault = defaultsByStrategy[selectedStrategy]
  const landingPromptIsDefault = selectedDefault !== undefined && landingPrompt === selectedDefault

  // Every open starts from what is actually stored, never from the previous
  // attempt. Keyed off the dialog opening rather than the prop: a
  // `repository_updated` off the stream mid-edit must not wipe what was typed.
  //
  // A new repository has no stored briefing to start from, so it opens blank
  // and the effect below seeds it with the selected strategy's default once
  // the strategies query answers — deliberately not a dependency here: the
  // query settling after the user has already started typing must not reset
  // the whole form out from under them the way it would if this waited on it.
  const seededLandingPrompt = useRef(false)
  useEffect(() => {
    if (!open) return
    seededLandingPrompt.current = false
    if (repository) {
      reset({
        path: repository.path,
        base_branch: repository.base_branch,
        description: repository.description ?? "",
        merge_strategy: repository.merge_strategy,
        landing_prompt: repository.landing_prompt,
      })
      // Already real text, not a blank waiting to be filled in.
      seededLandingPrompt.current = true
      return
    }
    reset(EMPTY_VALUES)
  }, [open, repository, reset])

  useEffect(() => {
    if (!open || repository || seededLandingPrompt.current) return
    // The strategy picked before the query answered, not the form's opening
    // default: a user who already picked Pull request must not be seeded with
    // Direct's text once the query catches up.
    const initialDefault = defaultsByStrategy[selectedStrategy]
    if (initialDefault === undefined) return
    seededLandingPrompt.current = true
    // Typed words already in the field, from before the query answered, are
    // the user's — `getValues` rather than the watched value so this reads
    // what is there right now instead of a render this effect did not run on.
    if (getValues("landing_prompt") === "") setValue("landing_prompt", initialDefault)
  }, [open, repository, defaultsByStrategy, selectedStrategy, getValues, setValue])

  /**
   * The briefing follows the strategy only while it is still that strategy's
   * own default — the moment it is edited it is the user's text, and picking
   * a different strategy afterwards must leave it alone. Wired to the
   * select's `onValueChange` rather than a `watch` effect: a `reset` also
   * changes `merge_strategy`, and must not be mistaken for this.
   */
  function handleStrategyChange(next: string) {
    if (selectedDefault === undefined || landingPrompt !== selectedDefault) return
    const nextDefault = defaultsByStrategy[next as MergeStrategy]
    if (nextDefault !== undefined) setValue("landing_prompt", nextDefault, { shouldDirty: true })
  }

  function resetLandingPrompt() {
    if (selectedDefault === undefined) return
    setValue("landing_prompt", selectedDefault, { shouldDirty: true, shouldValidate: true })
  }

  async function submit(values: RepositoryFormValues) {
    const path = values.path.trim()
    const branch = values.base_branch.trim()
    const description = values.description.trim()
    const strategyDefault = defaultsByStrategy[values.merge_strategy]
    const isDefaultBriefing =
      strategyDefault !== undefined && values.landing_prompt === strategyDefault
    try {
      if (repository) {
        await updateRepository.mutateAsync({
          id: repository.id,
          body: {
            path,
            // Editing shows the stored branch, so it is always sent; the
            // daemon only re-validates the checkout when one of them moved.
            base_branch: branch,
            // Empty is how the daemon spells "clear the description".
            description,
            merge_strategy: values.merge_strategy,
            // Empty is also how it spells "no landing override": the strategy's
            // own text is what is left in force.
            landing_prompt: isDefaultBriefing ? "" : values.landing_prompt,
          },
        })
        toast.success("Repository updated", { description: path })
      } else {
        const created = await createRepository.mutateAsync({
          path,
          // Absent, not empty: that is what asks for the repo's current branch.
          base_branch: branch || null,
          description: description || null,
          merge_strategy: values.merge_strategy,
          // Omitted rather than sent back: the daemon's own default for the
          // strategy is exactly what this already is.
          landing_prompt: isDefaultBriefing ? undefined : values.landing_prompt,
        })
        toast.success("Repository registered", { description: created.path })
      }
      onOpenChange(false)
    } catch (error) {
      showFailure(error)
    }
  }

  /**
   * The daemon's refusal, on the field it is about.
   *
   * A 400 is one of four things — the path is not absolute, it is not a git
   * work tree, the branch is unknown, or the landing briefing names a
   * placeholder nothing fills in — and the message picks the field: a
   * placeholder complaint names one by spelling it "placeholder(s)", checked
   * first because its message also lists `{branch}` and `{base_branch}` among
   * what the briefing may use, which would otherwise misroute it. A 409 is
   * about the pair, and goes above the buttons.
   */
  function showFailure(error: unknown): void {
    if (ApiError.is(error) && error.status === 400) {
      const message = describeError(error)
      if (/placeholder/i.test(error.message)) {
        setError("landing_prompt", { message })
        return
      }
      setError(/branch/i.test(error.message) ? "base_branch" : "path", { message })
      return
    }
    setError("root", { message: describeError(error) })
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-lg"
        title={editing ? "Edit repository" : "Register repository"}
        description="A checkout goals can be created against. Task worktrees are branched off its base branch."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: `Could not ${editing ? "save" : "register"} the repository`,
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel={editing ? "Save changes" : "Register repository"}
        pending={saving}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.path ? true : undefined}>
            <FieldLabel htmlFor="repository-path">Path</FieldLabel>
            <Input
              id="repository-path"
              placeholder="/absolute/path/to/repo"
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={formState.errors.path ? true : undefined}
              {...register("path")}
            />
            {formState.errors.path ? (
              <FieldError errors={[formState.errors.path]} />
            ) : (
              <FieldDescription>
                Absolute path to an existing git work tree on this machine.
              </FieldDescription>
            )}
          </Field>

          <Field data-invalid={formState.errors.base_branch ? true : undefined}>
            <FieldLabel htmlFor="repository-base-branch">Base branch</FieldLabel>
            <Input
              id="repository-base-branch"
              placeholder={editing ? "main" : "the repo's current branch"}
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={formState.errors.base_branch ? true : undefined}
              {...register("base_branch")}
            />
            {formState.errors.base_branch ? (
              <FieldError errors={[formState.errors.base_branch]} />
            ) : (
              <FieldDescription>
                {editing
                  ? "What task branches are cut from and merged back into."
                  : "Leave empty for whatever the repo has checked out right now."}
              </FieldDescription>
            )}
          </Field>

          <Field>
            <FieldLabel htmlFor="repository-merge-strategy">Merge strategy</FieldLabel>
            <FormSelect
              control={form.control}
              name="merge_strategy"
              id="repository-merge-strategy"
              options={MERGE_STRATEGIES}
              empty="direct"
              onValueChange={handleStrategyChange}
              // The name is what is picked; what it does is beside it, so the
              // choice is made in the list rather than after it.
              // `whitespace-normal` because the list's items are nowrap by
              // default and the sentence is a sentence: without it the popup,
              // which is exactly as wide as the trigger, would cut it off.
              renderOption={(option) => (
                <span className="flex min-w-0 flex-col whitespace-normal">
                  <span>{option.label}</span>
                  <span className="text-xs text-muted-foreground">
                    {MERGE_STRATEGY_META[option.value as MergeStrategy].description}
                  </span>
                </span>
              )}
            />
            <FieldDescription>{MERGE_STRATEGY_META[selectedStrategy].description}</FieldDescription>
          </Field>

          <Field data-invalid={formState.errors.landing_prompt ? true : undefined}>
            <div className="flex items-center justify-between gap-2">
              <FieldLabel htmlFor="repository-landing-prompt">Landing briefing</FieldLabel>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={resetLandingPrompt}
                disabled={selectedDefault === undefined || landingPromptIsDefault}
              >
                <RotateCcwIcon />
                Reset to default
              </Button>
            </div>
            <Textarea
              id="repository-landing-prompt"
              spellCheck={false}
              className="min-h-48 resize-y font-mono text-xs leading-relaxed"
              aria-invalid={formState.errors.landing_prompt ? true : undefined}
              {...register("landing_prompt")}
            />
            {formState.errors.landing_prompt ? (
              <FieldError errors={[formState.errors.landing_prompt]} />
            ) : (
              <FieldDescription>
                {landingPromptIsDefault
                  ? `The default briefing of the ${MERGE_STRATEGY_META[selectedStrategy].label.toLowerCase()} strategy.`
                  : "Customized for this repository."}{" "}
                Handed to the engineer of an approved task; may use {"{task_title}"}, {"{branch}"},{" "}
                {"{base_branch}"} and {"{repo_path}"}.
              </FieldDescription>
            )}
          </Field>

          <Field>
            <FieldLabel htmlFor="repository-description">Description</FieldLabel>
            <Textarea
              id="repository-description"
              placeholder="What lives in this repo."
              {...register("description")}
            />
            <FieldDescription>
              Optional. Shown next to the path wherever the repo is picked.
            </FieldDescription>
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
