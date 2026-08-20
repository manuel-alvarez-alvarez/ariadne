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

import { ApiError, type RepositoryDto } from "@/api"
import { FormDialog } from "@/components/form-dialog"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { describeError } from "@/lib/errors"

import { useCreateRepository, useUpdateRepository } from "./queries"

const formSchema = z.object({
  path: z
    .string()
    .trim()
    .min(1, "Enter the repository path.")
    .refine((value) => value.startsWith("/"), { message: "The path must be absolute." }),
  base_branch: z.string().trim(),
  description: z.string(),
})

type RepositoryFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: RepositoryFormValues = { path: "", base_branch: "", description: "" }

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
  const { formState, handleSubmit, register, reset, setError } = form

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
          },
        })
        toast.success("Repository updated", { description: path })
      } else {
        const created = await createRepository.mutateAsync({
          path,
          // Absent, not empty: that is what asks for the repo's current branch.
          base_branch: branch || null,
          description: description || null,
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
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={handleSubmit(submit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit repository" : "Register repository"}</DialogTitle>
            <DialogDescription>
              A checkout goals can be created against. Task worktrees are branched off its base
              branch.
            </DialogDescription>
          </DialogHeader>

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

          {formState.errors.root ? (
            <Alert variant="destructive">
              <AlertTitle>Could not {editing ? "save" : "register"} the repository</AlertTitle>
              <AlertDescription>{formState.errors.root.message}</AlertDescription>
            </Alert>
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" disabled={saving} />}>
              Cancel
            </DialogClose>
            <Button type="submit" pending={saving}>
              {editing ? "Save changes" : "Register repository"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </FormDialog>
  )
}
