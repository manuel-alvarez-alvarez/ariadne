/**
 * Create and edit dialog for a learned approval — one form for both, the way
 * the repository dialog is: creating picks the repository the approval
 * belongs to (`POST /v1/permissions/learned`), editing changes only the tool
 * name and the kind (`PUT /v1/permissions/learned/{id}`), since the contract
 * gives the repository no way to move once the row exists.
 *
 * A refusal is toasted with the daemon's own message rather than shown on a
 * field, the way the AI card's controls and the agents screen's flag editor
 * do: the fields here are opaque strings the daemon alone can judge, so there
 * is no field of this form a 409 or a 422 is naturally "about".
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useEffect } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import type { LearnedPermissionDto, RepositoryDto } from "@/api"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { describeError, folderName } from "@/lib/format"

import { useCreateLearnedPermission, useUpdateLearnedPermission } from "./queries"

const formSchema = z.object({
  repository_id: z.string().min(1, "Pick a repository."),
  tool_name: z.string().trim().min(1, "Enter the tool name."),
  kind: z.string().trim().min(1, "Enter the kind."),
})

type LearnedPermissionFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: LearnedPermissionFormValues = { repository_id: "", tool_name: "", kind: "" }

export function LearnedPermissionFormDialog({
  open,
  onOpenChange,
  editing,
  repositories,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The approval being edited, or null to add one. */
  editing: LearnedPermissionDto | null
  /** Every registered repository, for the create form's picker. */
  repositories: RepositoryDto[]
}) {
  const isEditing = editing !== null
  const createLearned = useCreateLearnedPermission()
  const updateLearned = useUpdateLearnedPermission()
  const saving = createLearned.isPending || updateLearned.isPending

  const form = useForm<LearnedPermissionFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { control, formState, handleSubmit, register, reset } = form

  // Every open starts from what is actually stored, never from the previous
  // attempt. Keyed off the dialog opening rather than the prop: a
  // `learned_permission_updated` off the stream mid-edit must not wipe what
  // was typed.
  useEffect(() => {
    if (!open) return
    if (editing) {
      reset({
        repository_id: editing.repository_id,
        tool_name: editing.tool_name,
        kind: editing.kind,
      })
      return
    }
    reset(EMPTY_VALUES)
  }, [open, editing, reset])

  function submit(values: LearnedPermissionFormValues) {
    const toolName = values.tool_name.trim()
    const kind = values.kind.trim()
    if (editing) {
      updateLearned.mutate(
        { id: editing.id, body: { tool_name: toolName, kind } },
        {
          onSuccess: () => {
            toast.success("Approval updated", { description: toolName })
            onOpenChange(false)
          },
          onError: (error) =>
            toast.error("Could not update the approval", { description: describeError(error) }),
        },
      )
      return
    }
    createLearned.mutate(
      { repository_id: values.repository_id, tool_name: toolName, kind },
      {
        onSuccess: (created) => {
          toast.success("Approval added", { description: created.tool_name })
          onOpenChange(false)
        },
        onError: (error) =>
          toast.error("Could not add the approval", { description: describeError(error) }),
      },
    )
  }

  const editingRepositoryPath =
    editing &&
    (repositories.find((repository) => repository.id === editing.repository_id)?.path ??
      editing.repository_id)

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-md"
        title={isEditing ? "Edit approval" : "Add approval"}
        description={
          isEditing
            ? "Change what this approval matches. It stays with the repository it was added for."
            : "A tool a repository's agents may run without being asked again."
        }
        onSubmit={handleSubmit(submit)}
        submitLabel={isEditing ? "Save changes" : "Add approval"}
        pending={saving}
      >
        <FormDialogBody>
          {isEditing ? (
            <Field>
              <FieldLabel>Repository</FieldLabel>
              <p className="truncate font-mono text-sm text-muted-foreground">
                {editingRepositoryPath}
              </p>
            </Field>
          ) : (
            <Field data-invalid={formState.errors.repository_id ? true : undefined}>
              <FieldLabel htmlFor="learned-repository">Repository</FieldLabel>
              <Controller
                control={control}
                name="repository_id"
                render={({ field }) => {
                  const selected = repositories.find((repository) => repository.id === field.value)
                  return (
                    <Select
                      value={field.value}
                      onValueChange={field.onChange}
                      items={repositories.map((repository) => ({
                        value: repository.id,
                        label: folderName(repository.path),
                      }))}
                    >
                      <SelectTrigger
                        id="learned-repository"
                        aria-label="Repository"
                        className="w-full"
                        aria-invalid={formState.errors.repository_id ? true : undefined}
                        onBlur={field.onBlur}
                        title={selected?.path}
                      >
                        <SelectValue placeholder="Pick a repository…">
                          {selected ? folderName(selected.path) : undefined}
                        </SelectValue>
                      </SelectTrigger>
                      {/* Wide enough for a full path, decoupled from the
                          trigger's own width — see `learned-tab.tsx`. */}
                      <SelectContent alignItemWithTrigger={false} className="w-96">
                        {repositories.map((repository) => (
                          <SelectItem
                            key={repository.id}
                            value={repository.id}
                            aria-label={repository.path}
                          >
                            <span className="flex min-w-0 flex-col overflow-hidden py-0.5">
                              <span className="truncate">{folderName(repository.path)}</span>
                              <span className="truncate text-xs text-muted-foreground">
                                {repository.path}
                              </span>
                            </span>
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  )
                }}
              />
              {formState.errors.repository_id ? (
                <FieldError errors={[formState.errors.repository_id]} />
              ) : null}
            </Field>
          )}

          <Field data-invalid={formState.errors.tool_name ? true : undefined}>
            <FieldLabel htmlFor="learned-tool-name">Tool name</FieldLabel>
            <Input
              id="learned-tool-name"
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={formState.errors.tool_name ? true : undefined}
              {...register("tool_name")}
            />
            {formState.errors.tool_name ? (
              <FieldError errors={[formState.errors.tool_name]} />
            ) : (
              <FieldDescription>The tool call's own name, e.g. “Bash”.</FieldDescription>
            )}
          </Field>

          <Field data-invalid={formState.errors.kind ? true : undefined}>
            <FieldLabel htmlFor="learned-kind">Kind</FieldLabel>
            <Input
              id="learned-kind"
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              placeholder="execute"
              aria-invalid={formState.errors.kind ? true : undefined}
              {...register("kind")}
            />
            {formState.errors.kind ? (
              <FieldError errors={[formState.errors.kind]} />
            ) : (
              <FieldDescription>The tool call's own kind, e.g. “execute”.</FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
