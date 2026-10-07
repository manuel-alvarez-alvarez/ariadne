/**
 * Create and edit dialog for a repository — one form for both, because the two
 * differ only in where they post and in what an omitted base branch means.
 *
 * A repository is a checkout, a base branch, how its agents' permission
 * requests are answered, and the landing a new goal against it uses where the
 * goal's own request leaves landing out. How work ends for a given task or
 * goal is still agreed with the user there, not here — this is only the
 * default that choice starts from.
 *
 * Editing a repository whose remote is on GitHub or GitLab also shows the
 * forge the daemon detected there, and the integration with it (spec 025):
 * only what changed of it is sent, since an absent field is one the daemon
 * leaves alone. Registering shows none of it — nothing is detected until the
 * daemon has opened the checkout.
 *
 * The client only catches what it can know on its own: a missing or relative
 * path. Everything else is the daemon's to say — it opens the checkout and
 * resolves the branch — so a 400 lands on the field it is about (the path, or
 * the branch when one was typed) and a 409 says the pair is already
 * registered.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { XIcon } from "lucide-react"
import { useEffect } from "react"
import {
  type Control,
  Controller,
  type FieldError as FormFieldError,
  useForm,
} from "react-hook-form"
import { toast } from "sonner"
import { z } from "zod"

import { ApiError, type ForgeDto, type ModelDto, type RepositoryDto } from "@/api"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Button } from "@/components/ui/button"
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
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { describeError, LANDING_ITEMS } from "@/lib/format"
import { forgeChanges, forgeKindLabel, forgeRepositoryLabel, forgeValues } from "./forge"
import { PERMISSION_MODES } from "./permission-modes"
import { useCreateRepository, useUpdateRepository } from "./queries"
import { WebhookStatus } from "./webhook-status"

const PERMISSION_ITEMS = PERMISSION_MODES.map(({ value, label }) => ({ value, label }))

const formSchema = z.object({
  path: z
    .string()
    .trim()
    .min(1, "Enter the repository path.")
    .refine((value) => value.startsWith("/"), { message: "The path must be absolute." }),
  base_branch: z.string().trim(),
  description: z.string(),
  permission_mode: z.enum(["auto", "ask", "learn", "ai"]),
  default_landing: z.enum(["merge", "pull_request", "none", "feature_branch"]),
  forge_enabled: z.boolean(),
  babysit_model: z.string(),
  babysit_effort: z.string(),
  review_model: z.string(),
  review_effort: z.string(),
})

type RepositoryFormValues = z.infer<typeof formSchema>

const EMPTY_VALUES: RepositoryFormValues = {
  path: "",
  base_branch: "",
  description: "",
  permission_mode: "auto",
  default_landing: "merge",
  ...forgeValues(null),
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
  const saving = createRepository.isPending || updateRepository.isPending
  const forge = repository?.forge
  // The catalog the two role pickers offer, read only where they show.
  const models = useQuery({ ...modelsQueryOptions(), enabled: open && Boolean(forge) })

  const form = useForm<RepositoryFormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { control, formState, handleSubmit, register, reset, setError } = form

  // Every open starts from what is actually stored, never from the previous
  // attempt. Keyed off the dialog opening rather than the prop: a
  // `repository_updated` off the stream mid-edit must not wipe what was typed.
  useEffect(() => {
    if (!open) return
    if (repository) {
      reset({
        path: repository.path,
        base_branch: repository.base_branch,
        description: repository.description ?? "",
        permission_mode: repository.permission_mode,
        default_landing: repository.default_landing,
        ...forgeValues(repository.forge),
      })
      return
    }
    reset(EMPTY_VALUES)
  }, [open, repository, reset])

  async function submit(values: RepositoryFormValues) {
    const path = values.path.trim()
    const branch = values.base_branch.trim()
    const description = values.description.trim()
    try {
      if (repository) {
        const forgeUpdate = repository.forge ? forgeChanges(repository.forge, values) : null
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
            default_landing: values.default_landing,
            ...(forgeUpdate ? { forge: forgeUpdate } : {}),
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
          default_landing: values.default_landing,
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
   * field of this form is what it is about, and neither is a pin the daemon
   * refused, which could be either role's.
   */
  function showFailure(error: unknown): void {
    if (
      ApiError.is(error) &&
      (error.code.startsWith("forge_") || /forge integration/.test(error.message))
    ) {
      setError("forge_enabled", { message: describeError(error) })
      return
    }
    if (ApiError.is(error) && error.status === 400 && /model|effort|agent/i.test(error.message)) {
      setError("root", { message: describeError(error) })
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

          <Field data-invalid={formState.errors.default_landing ? true : undefined}>
            <FieldLabel htmlFor="repository-default-landing">Default landing</FieldLabel>
            <FormSelect
              control={control}
              name="default_landing"
              id="repository-default-landing"
              options={LANDING_ITEMS}
            />
            {formState.errors.default_landing ? (
              <FieldError errors={[formState.errors.default_landing]} />
            ) : (
              <FieldDescription>
                How a new goal against this repository ends, where the goal's own request leaves
                landing out.
              </FieldDescription>
            )}
          </Field>

          {editing ? (
            <ForgeSection
              forge={forge}
              control={control}
              models={models.data}
              enabledError={formState.errors.forge_enabled}
            />
          ) : null}
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}

/** The two roles, as the form names their fields and the screen names them. */
const ROLES = [
  {
    model: "babysit_model",
    effort: "babysit_effort",
    label: "Babysitter runs on",
    id: "forge-babysit-pin",
    meaning: "Watches a published request: answers its comments and fixes its checks.",
  },
  {
    model: "review_model",
    effort: "review_effort",
    label: "Reviewer runs on",
    id: "forge-review-pin",
    meaning: "Reviews a request on the forge.",
  },
] as const

/**
 * The Forge section (spec 025): the remote the daemon detected, read-only, the
 * switch that enables the integration, and what each of its two roles runs on.
 * Enabling asks the forge's own CLI whether it is signed in to the host, so a
 * refusal comes back in the CLI's words and lands on the switch. A role left
 * without a model is allowed: it starts no session.
 */
function ForgeSection({
  forge,
  control,
  models,
  enabledError,
}: {
  forge: ForgeDto | null | undefined
  control: Control<RepositoryFormValues>
  models: ModelDto[] | undefined
  enabledError: FormFieldError | undefined
}) {
  if (!forge) {
    return (
      <Field>
        <FieldLabel>Forge</FieldLabel>
        <FieldDescription>
          No GitHub or GitLab remote detected. Ariadne reads <code>origin</code>, or the only remote
          where there is no <code>origin</code>.
        </FieldDescription>
      </Field>
    )
  }
  return (
    <fieldset className="flex flex-col gap-4 rounded-lg border p-3">
      <legend className="px-1 text-sm font-medium">Forge</legend>
      <p className="text-xs text-muted-foreground" data-testid="forge-remote">
        {forgeKindLabel(forge.kind)}{" "}
        <span className="font-mono text-foreground">{forgeRepositoryLabel(forge)}</span> via{" "}
        <span className="font-mono">{forge.remote}</span>
        {forge.enabled && forge.login ? ` · signed in as ${forge.login}` : null}
      </p>

      <WebhookStatus webhook={forge.webhook} />

      <Field data-invalid={enabledError ? true : undefined} orientation="horizontal">
        <Controller
          control={control}
          name="forge_enabled"
          render={({ field }) => (
            <Switch
              id="forge-enabled"
              aria-label="Enabled"
              checked={field.value}
              onCheckedChange={(checked) => field.onChange(checked)}
              aria-invalid={enabledError ? true : undefined}
            />
          )}
        />
        <FieldLabel htmlFor="forge-enabled">Enabled</FieldLabel>
      </Field>
      {enabledError ? (
        <FieldError errors={[enabledError]} />
      ) : (
        <FieldDescription>
          Needs <code>{forge.kind === "github" ? "gh" : "glab"}</code> signed in to {forge.host}.
        </FieldDescription>
      )}

      {ROLES.map((role) => (
        <Field key={role.model}>
          <FieldLabel htmlFor={role.id}>{role.label}</FieldLabel>
          <Controller
            control={control}
            name={role.model}
            render={({ field }) => (
              <Controller
                control={control}
                name={role.effort}
                render={({ field: effort }) => (
                  <div className="flex items-center gap-2">
                    <PinPicker
                      id={role.id}
                      label={role.label}
                      model={field.value}
                      effort={effort.value}
                      onChange={(pin) => {
                        field.onChange(pin.model)
                        effort.onChange(pin.effort)
                      }}
                      models={models}
                      className="flex-1"
                    />
                    {field.value ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Clear: ${role.label}`}
                        onClick={() => {
                          field.onChange("")
                          effort.onChange("")
                        }}
                      >
                        <XIcon />
                      </Button>
                    ) : null}
                  </div>
                )}
              />
            )}
          />
          <FieldDescription>
            {role.meaning} {NO_PIN_HINT}
          </FieldDescription>
        </Field>
      ))}
    </fieldset>
  )
}

/** The rule every role shares, said once under each picker. */
const NO_PIN_HINT = "No model starts no session."
