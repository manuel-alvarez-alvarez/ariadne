/**
 * Create and edit dialog for a repository — one form for both, because the two
 * differ only in where they post and in what an omitted base branch means.
 *
 * The client only catches what it can know on its own: a missing or relative
 * path. Everything else is the daemon's to say — it opens the checkout,
 * resolves the branch and checks it has commits — so a 400 lands on the field
 * it is about (the path, or the branch when one was typed) and a 409 says the
 * pair is already registered.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useEffect } from "react"
import { useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type MergeStrategy, type RepositoryDto } from "@/api"
import { FormDialog, FormDialogContent } from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { describeError } from "@/lib/format"

import { useCreateRepository, useUpdateRepository } from "./queries"

const formSchema = z.object({
  path: z
    .string()
    .trim()
    .min(1, "Enter the repository path.")
    .refine((value) => value.startsWith("/"), { message: "The path must be absolute." }),
  base_branch: z.string().trim(),
  description: z.string(),
  merge_strategy: z.enum(["direct", "pull_request"]),
})

type RepositoryFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: RepositoryFormValues = {
  path: "",
  base_branch: "",
  description: "",
  merge_strategy: "direct",
}

/**
 * How an approved task reaches the base branch here, in the two words the
 * daemon knows and the sentence each of them means for the engineer that has
 * to act on it.
 */
const MERGE_STRATEGIES: { value: MergeStrategy; label: string; hint: string }[] = [
  {
    value: "direct",
    label: "Squash onto the base branch",
    hint: "The engineer rebases, squashes the task into one commit and fast-forwards the base branch itself.",
  },
  {
    value: "pull_request",
    label: "Publish a pull or merge request",
    hint: "The engineer opens a request with `gh` or `glab`, answers what is written on it, and finishes the task once it is merged.",
  },
]

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

  const form = useForm<RepositoryFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { formState, handleSubmit, register, reset, setError, watch } = form
  const selectedStrategy = watch("merge_strategy")

  // Every open starts from what is actually stored, never from the previous
  // attempt. Keyed off the dialog opening rather than the prop: a
  // `repository_updated` off the stream mid-edit must not wipe what was typed.
  useEffect(() => {
    if (!open) return
    reset(
      repository
        ? {
            path: repository.path,
            base_branch: repository.base_branch,
            description: repository.description ?? "",
            merge_strategy: repository.merge_strategy,
          }
        : EMPTY_VALUES,
    )
  }, [open, repository, reset])

  async function submit(values: RepositoryFormValues) {
    const path = values.path.trim()
    const branch = values.base_branch.trim()
    const description = values.description.trim()
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
   * A 400 is one of three things — the path is not absolute, it is not a git
   * work tree, or the branch is unknown — and only the last of those is about
   * the branch, so the message picks the field. A 409 is about the pair, and
   * goes above the buttons.
   */
  function showFailure(error: unknown): void {
    if (ApiError.is(error) && error.status === 400) {
      const message = describeError(error)
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
        <FieldGroup>
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
              options={MERGE_STRATEGIES.map(({ value, label }) => ({ label, value }))}
              empty="direct"
            />
            <FieldDescription>
              {MERGE_STRATEGIES.find(({ value }) => value === selectedStrategy)?.hint}
            </FieldDescription>
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
        </FieldGroup>
      </FormDialogContent>
    </FormDialog>
  )
}
