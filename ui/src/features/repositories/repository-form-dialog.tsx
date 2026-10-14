/**
 * Create and edit dialog for a repository — one form for both, because the two
 * differ only in where they post and in what an omitted base branch means.
 *
 * A repository is a checkout, a base branch, how its agents' permission
 * requests are answered, and the workflow a new goal against it runs where the
 * goal's own request names none. Which workflow a given goal runs is still
 * agreed with the user there, not here — this is only the default that choice
 * starts from.
 *
 * A repository whose remote is on GitHub or GitLab also shows one switch for
 * the integration with that forge (spec 025). Editing sends it only when it
 * moved, since an absent field is one the daemon leaves alone. Registering
 * probes `POST /v1/forge/detect` itself, off the path field, and sends it
 * only once that probe actually found one; a checkout with no such remote
 * shows neither there nor once registered. "Supervise posts" sits right
 * below the switch and only while it is on, in both forms.
 *
 * The client only catches what it can know on its own: a missing or relative
 * path. Everything else is the daemon's to say — it opens the checkout and
 * resolves the branch — so a 400 lands on the field it is about (the path, or
 * the branch when one was typed) and a 409 says the pair is already
 * registered.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useCallback, useEffect, useRef, useState } from "react"
import {
  type Control,
  Controller,
  type FieldError as FormFieldError,
  useForm,
  useWatch,
} from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type ForgeDetectionDto, type ForgeDto, type RepositoryDto } from "@/api"
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
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { pickedWorkflow, WorkflowSelect } from "@/features/workflows/workflow-select"
import { describeError } from "@/lib/format"
import { forgeCliName, forgeKindLabel, forgeRepositoryLabel } from "./forge"
import { PERMISSION_MODES } from "./permission-modes"
import { useCreateRepository, useDetectForge, useUpdateRepository } from "./queries"

const PERMISSION_ITEMS = PERMISSION_MODES.map(({ value, label }) => ({ value, label }))

/** How long the path field sits still before a detection fires on its own. */
const DETECT_SETTLES_MS = 400

/** Whether two probes of `POST /v1/forge/detect` answer the same forge, or
 * both answer none — a repeated probe of a path nothing changed about. */
function sameDetection(a: ForgeDetectionDto | null, b: ForgeDetectionDto | null): boolean {
  if (a === null || b === null) return a === b
  return (
    a.kind === b.kind &&
    a.host === b.host &&
    a.owner === b.owner &&
    a.name === b.name &&
    a.remote === b.remote
  )
}

const formSchema = z.object({
  path: z
    .string()
    .trim()
    .min(1, "Enter the repository path.")
    .refine((value) => value.startsWith("/"), { message: "The path must be absolute." }),
  base_branch: z.string().trim(),
  description: z.string(),
  permission_mode: z.enum(["auto", "ask", "learn", "ai"]),
  supervise_posts: z.boolean(),
  default_workflow: z.string().min(1, "Pick a workflow."),
  forge_enabled: z.boolean(),
})

type RepositoryFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: RepositoryFormValues = {
  path: "",
  base_branch: "",
  description: "",
  permission_mode: "auto",
  supervise_posts: true,
  default_workflow: "",
  forge_enabled: false,
}

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
  const detectForge = useDetectForge()
  const saving = createRepository.isPending || updateRepository.isPending

  const form = useForm<RepositoryFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { control, formState, handleSubmit, register, reset, setError, setValue } = form
  const forgeEnabled = useWatch({ control, name: "forge_enabled" })

  // Registering probes the path itself — nothing is detected server-side
  // until the daemon has opened the checkout. Editing skips this: the
  // repository already carries what the daemon detected, in `forge`.
  const [detected, setDetected] = useState<ForgeDetectionDto | null>(null)
  const detectedRef = useRef<ForgeDetectionDto | null>(null)
  const detectRequest = useRef(0)
  const detectTimeout = useRef<ReturnType<typeof setTimeout> | null>(null)

  const probe = useCallback(
    (path: string) => {
      if (editing) return
      const request = ++detectRequest.current
      // A changed detection — on, off, or none — answers a switch left on
      // for the checkout before it, so that must not survive it. A probe
      // that only repeats the one already settled — the blur a click on
      // the switch itself fires, not a typed change — must leave the
      // switch exactly as the person just set it.
      const settle = (result: ForgeDetectionDto | null) => {
        if (request !== detectRequest.current) return
        if (!sameDetection(result, detectedRef.current)) {
          setValue("forge_enabled", false)
        }
        detectedRef.current = result
        setDetected(result)
      }
      if (!path.startsWith("/")) {
        settle(null)
        return
      }
      detectForge.mutate({ path }, { onSuccess: settle, onError: () => settle(null) })
    },
    [editing, detectForge, setValue],
  )

  const onPathChange = useCallback(
    (path: string) => {
      clearTimeout(detectTimeout.current ?? undefined)
      detectTimeout.current = setTimeout(() => probe(path), DETECT_SETTLES_MS)
    },
    [probe],
  )

  const onPathBlur = useCallback(
    (path: string) => {
      clearTimeout(detectTimeout.current ?? undefined)
      probe(path)
    },
    [probe],
  )

  useEffect(() => () => clearTimeout(detectTimeout.current ?? undefined), [])

  const forge = editing ? (repository?.forge ?? null) : detected

  // Every open starts from what is actually stored, never from the previous
  // attempt. Keyed off the dialog opening rather than the prop: a
  // `repository_updated` off the stream mid-edit must not wipe what was typed.
  useEffect(() => {
    if (!open) return
    detectRequest.current++
    clearTimeout(detectTimeout.current ?? undefined)
    setDetected(null)
    detectedRef.current = null
    if (repository) {
      reset({
        path: repository.path,
        base_branch: repository.base_branch,
        description: repository.description ?? "",
        permission_mode: repository.permission_mode,
        supervise_posts: repository.supervise_posts,
        default_workflow: repository.default_workflow ?? "",
        forge_enabled: repository.forge?.enabled ?? false,
      })
      return
    }
    reset(EMPTY_VALUES)
  }, [open, repository, reset])

  async function submit(values: RepositoryFormValues) {
    const path = values.path.trim()
    const branch = values.base_branch.trim()
    const description = values.description.trim()
    const workflow = pickedWorkflow(values.default_workflow)
    try {
      if (repository) {
        const forgeMoved = repository.forge && values.forge_enabled !== repository.forge.enabled
        await updateRepository.mutateAsync({
          id: repository.id,
          body: {
            path,
            // Editing shows the stored branch, so it is always sent; the
            // daemon only re-validates the checkout when one of them moved.
            base_branch: branch,
            // Empty is how the daemon spells "clear the description".
            description,
            permission_mode: values.permission_mode,
            supervise_posts: values.supervise_posts,
            default_workflow: workflow,
            ...(forgeMoved ? { forge: { enabled: values.forge_enabled } } : {}),
          },
        })
        toast.success("Repository updated", { description: path })
      } else {
        const created = await createRepository.mutateAsync({
          path,
          // Absent, not empty: that is what asks for the repo's current branch.
          base_branch: branch || null,
          description: description || null,
          permission_mode: values.permission_mode,
          supervise_posts: values.supervise_posts,
          default_workflow: workflow,
          ...(detected ? { forge: { enabled: values.forge_enabled } } : {}),
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
   * work tree, or the branch is unknown — and the message picks the field.
   * `ai_disabled` is about the mode just picked, so it lands there too,
   * pointing at the screen that turns the model on rather than repeating the
   * daemon's own CLI-flavoured words. A refusal to enable the forge — its CLI
   * is not signed in, or another registration of the checkout holds it — is
   * the switch's. A 409 about the pair goes above the buttons instead — no
   * field of this form is what it is about.
   */
  function showFailure(error: unknown): void {
    if (
      ApiError.is(error) &&
      (error.code.startsWith("forge_") || /forge integration/.test(error.message))
    ) {
      setError("forge_enabled", { message: describeError(error) })
      return
    }
    if (ApiError.is(error) && error.status === 400) {
      const message = describeError(error)
      setError(/branch/i.test(error.message) ? "base_branch" : "path", { message })
      return
    }
    if (ApiError.is(error) && error.code === "ai_disabled") {
      setError("permission_mode", {
        message: "Enable the AI permission model on the Permissions screen first",
      })
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
              {...register("path", {
                onChange: (e) => onPathChange(e.target.value),
                onBlur: (e) => onPathBlur(e.target.value),
              })}
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

          <Field data-invalid={formState.errors.permission_mode ? true : undefined}>
            <FieldLabel htmlFor="repository-permission-mode">Permission requests</FieldLabel>
            <Controller
              control={control}
              name="permission_mode"
              render={({ field }) => (
                <Select
                  value={field.value}
                  onValueChange={(value) => field.onChange(value)}
                  // Without this the trigger shows the stored value (`learn`)
                  // rather than the option's label ("Learn").
                  items={PERMISSION_ITEMS}
                >
                  <SelectTrigger
                    id="repository-permission-mode"
                    aria-label="Permission requests"
                    className="w-full"
                    aria-invalid={formState.errors.permission_mode ? true : undefined}
                    onBlur={field.onBlur}
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent alignItemWithTrigger={false}>
                    {PERMISSION_MODES.map((mode) => (
                      <SelectItem key={mode.value} value={mode.value}>
                        <span className="flex flex-col py-0.5">
                          <span>{mode.label}</span>
                          <span className="text-muted-foreground text-xs whitespace-normal">
                            {mode.meaning}
                          </span>
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            />
            {formState.errors.permission_mode ? (
              <FieldError errors={[formState.errors.permission_mode]} />
            ) : (
              <FieldDescription>
                How every agent working in this checkout has its tool permission requests answered.
              </FieldDescription>
            )}
          </Field>

          <Field data-invalid={formState.errors.default_workflow ? true : undefined}>
            <FieldLabel htmlFor="repository-default-workflow">Default workflow</FieldLabel>
            <WorkflowSelect
              control={control}
              name="default_workflow"
              id="repository-default-workflow"
              enabled={open}
            />
            {formState.errors.default_workflow ? (
              <FieldError errors={[formState.errors.default_workflow]} />
            ) : (
              <FieldDescription>
                The workflow a new goal against this repository runs, where the goal names none.
              </FieldDescription>
            )}
          </Field>

          {forge ? (
            <ForgeSwitch forge={forge} control={control} error={formState.errors.forge_enabled} />
          ) : null}

          {forge && forgeEnabled ? (
            <Field>
              <div className="flex items-center gap-3">
                <Controller
                  control={control}
                  name="supervise_posts"
                  render={({ field }) => (
                    <Switch
                      id="repository-supervise-posts"
                      aria-label="Supervise posts"
                      checked={field.value}
                      onCheckedChange={(checked) => field.onChange(checked)}
                    />
                  )}
                />
                <FieldLabel htmlFor="repository-supervise-posts">Supervise posts</FieldLabel>
              </div>
              <FieldDescription>
                Put agent replies, reviews and resolves in your pending GitHub review or GitLab
                draft notes, and open pull requests as drafts.
              </FieldDescription>
            </Field>
          ) : null}
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}

/**
 * The integration with the forge the daemon detected on the remote (spec 025),
 * as one switch. Enabling asks the forge's own CLI whether it is signed in to
 * the host, so a refusal comes back in the CLI's words and lands here.
 *
 * `forge` is a stored `ForgeDto` once the repository is registered, or the
 * leaner `ForgeDetectionDto` the create form's own probe answers with ahead
 * of that — neither `enabled` nor `login` apply there yet.
 */
function ForgeSwitch({
  forge,
  control,
  error,
}: {
  forge: ForgeDto | ForgeDetectionDto
  control: Control<RepositoryFormValues>
  error: FormFieldError | undefined
}) {
  const label = `${forgeKindLabel(forge.kind)} integration`
  return (
    <Field data-invalid={error ? true : undefined}>
      {/* The switch leads the row: its hit area reaches past its edge, and at
          the row's far end that is past the scrolling body's, which then
          scrolls sideways by that much. */}
      <div className="flex items-center gap-3">
        <Controller
          control={control}
          name="forge_enabled"
          render={({ field }) => (
            <Switch
              id="forge-enabled"
              aria-label={label}
              checked={field.value}
              onCheckedChange={(checked) => field.onChange(checked)}
              aria-invalid={error ? true : undefined}
            />
          )}
        />
        <FieldLabel htmlFor="forge-enabled">{label}</FieldLabel>
      </div>
      {error ? (
        <FieldError errors={[error]} />
      ) : (
        <FieldDescription data-testid="forge-remote">
          <span className="font-mono">{forgeRepositoryLabel(forge)}</span>, via{" "}
          <code>{forgeCliName(forge.kind)}</code>
          {"enabled" in forge && forge.enabled && forge.login
            ? `, signed in as ${forge.login}`
            : ""}
          . Lists its pull requests and issues, and keeps them up to date.
        </FieldDescription>
      )}
    </Field>
  )
}
