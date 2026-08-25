/**
 * Create and edit dialog for a profile — one form for both, because the two
 * differ only in where they post and in the one field the daemon cannot change
 * after creation (the role).
 *
 * Every prompt the profile is spawned with is edited here, not just the system
 * one: the role's briefings are folded into the same form, filled with what the
 * profile is briefed with — its own text where it has one, the default of the
 * kind where it has none, each said to be which. This is the only place any of
 * them is edited or restored; the details panel shows them read-only.
 *
 * The two modes differ in what there is to show. A profile that does not exist
 * yet owns no prompts to read, and the defaults are the daemon's text with no
 * endpoint handing them out, so creating offers the system prompt alone —
 * blank meaning "the role's own" — and the briefings are edited once the
 * profile is there. Editing shows every one of them.
 *
 * Writing is a request per prompt either way: the profile's own fields go in
 * its `POST`/`PUT`, and every changed briefing is a `PUT` of its own
 * afterwards. Those can fail separately, so a failure leaves the dialog open,
 * says which write failed, and remembers what already landed so a retry does
 * not repeat it. Restoring a default is the one thing written the moment it is
 * asked for: this form holds no copy of a default to put in the box.
 *
 * Validation is client-side for the two required fields and daemon-side for
 * everything it alone can know: a duplicate name comes back as a 409 and is
 * shown on the name field, anything else as an alert above the buttons.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { useEffect, useRef, useState } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type ProfileDto, type UpdateProfileRequest } from "@/api"
import { FormDialog, FormDialogContent } from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { describeError, ROLE_LABELS } from "@/lib/format"
import { ModelCombobox } from "./model-combobox"
import {
  AGENT_KIND_CHOICES,
  type AgentKindChoice,
  AUTO_AGENT_KIND,
  changedPrompts,
  emptyProfileFormValues,
  type ProfileFormValues,
  profileFormSchema,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"
import { agentKindLabel, promptKindLabel, ROLES } from "./profile-labels"
import { ProfilePromptsField, replacePrompt, type SavedState } from "./profile-prompts-field"
import {
  modelsQueryOptions,
  profilePromptsQueryOptions,
  useCreateProfile,
  useUpdateProfile,
  useUpdateProfilePrompt,
} from "./queries"

export function ProfileFormDialog({
  open,
  onOpenChange,
  profile,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The profile being edited, or null to create a new one. */
  profile: ProfileDto | null
}) {
  const editing = profile !== null
  const createProfile = useCreateProfile()
  const updateProfile = useUpdateProfile()
  const updatePrompt = useUpdateProfilePrompt()
  const saving = createProfile.isPending || updateProfile.isPending || updatePrompt.isPending

  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: emptyProfileFormValues(),
  })
  const { control, formState, handleSubmit, register, reset, setError, setValue, watch } = form

  /**
   * Whether the profile is on its role's default system prompt.
   *
   * Kept here rather than read off the `profile` prop because a restore
   * changes it while the dialog is open, and the prop is whatever the list
   * last handed down.
   */
  const [systemIsDefault, setSystemIsDefault] = useState(true)
  /** Bumped on every open, to remount the prompts field with nothing unfolded. */
  const [promptsKey, setPromptsKey] = useState(0)
  const saved = useRef<SavedState>({ profile: null, prompts: [] })
  /** What the form was last reset for, so a re-render does not undo edits. */
  const openedFor = useRef<string | null>(null)
  /** Whether the prompt editors have been filled from the profile's own. */
  const seededFrom = useRef<string | null>(null)

  const model = watch("model")
  const agentKind = watch("agentKind")

  // The catalog behind the model combobox. Only asked for while the dialog is
  // up, and allowed to fail: an undefined catalog leaves the field free-text.
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })

  // What this profile is briefed with, which is only a question when editing:
  // its own text where it has one, the default of the kind where it has none.
  const stored = useQuery({
    ...profilePromptsQueryOptions(profile?.id ?? ""),
    enabled: open && editing,
  })

  // Every open starts from what is actually stored, never a stale draft. Keyed
  // by what is being edited rather than by the prop: a write of one prompt
  // re-renders this dialog, and a reset then would drop the unsaved others.
  useEffect(() => {
    if (!open) {
      openedFor.current = null
      seededFrom.current = null
      return
    }
    const key = profile ? profile.id : "new"
    if (openedFor.current === key) return
    openedFor.current = key
    seededFrom.current = null
    const values = profile ? profileToFormValues(profile) : emptyProfileFormValues()
    reset(values)
    // A fresh prompts field, so nothing stays folded open from the last time
    // this dialog was up.
    setPromptsKey((key) => key + 1)
    setSystemIsDefault(profile ? profile.system_prompt_is_default : true)
    saved.current = { profile: profile ? toUpdateRequest(values) : null, prompts: [] }
  }, [open, profile, reset])

  // Editing: the briefings are what the profile is briefed with, and they are
  // also the baseline every later submit is diffed against — so a default read
  // and left alone is never written back as an override.
  const storedData = stored.data
  useEffect(() => {
    if (!open || !editing || !storedData || seededFrom.current === "stored") return
    seededFrom.current = "stored"
    const values = storedData.map((prompt) => ({ kind: prompt.kind, content: prompt.content }))
    setValue("prompts", values)
    saved.current.prompts = values
  }, [open, editing, storedData, setValue])

  async function create(values: ProfileFormValues) {
    const created = await createProfile.mutateAsync(toCreateRequest(values))
    toast.success("Profile created", { description: created.name })
    onOpenChange(false)
  }

  /**
   * The profile, then each changed briefing, one request at a time.
   *
   * Sequential rather than concurrent so a failure has an unambiguous story:
   * everything named before it is stored, everything after it is not.
   */
  async function save(target: ProfileDto, values: ProfileFormValues) {
    const body = toUpdateRequest(values)
    if (!sameProfileBody(saved.current.profile, body)) {
      try {
        const updated = await updateProfile.mutateAsync({
          id: target.id,
          body: withoutUnchangedSystemPrompt(body, saved.current.profile),
        })
        saved.current.profile = body
        setSystemIsDefault(updated.system_prompt_is_default)
      } catch (error) {
        if (ApiError.is(error) && error.status === 409) {
          setError("name", { message: `A profile named "${values.name.trim()}" already exists.` })
          return
        }
        setError("root", {
          message: `The profile itself could not be saved: ${describeError(error)}`,
        })
        return
      }
    }

    for (const prompt of changedPrompts(values.prompts, saved.current.prompts)) {
      const label = promptKindLabel(prompt.kind).toLowerCase()
      try {
        await updatePrompt.mutateAsync({ id: target.id, ...prompt })
        saved.current.prompts = replacePrompt(saved.current.prompts, prompt)
      } catch (error) {
        setError("root", {
          message: `The ${label} could not be saved: ${describeError(error)} Everything saved before it is already stored; submitting again retries only what is left.`,
        })
        return
      }
    }

    toast.success("Profile updated", { description: values.name.trim() })
    onOpenChange(false)
  }

  async function submit(values: ProfileFormValues) {
    try {
      if (profile) {
        await save(profile, values)
      } else {
        await create(values)
      }
    } catch (error) {
      // A name clash is the one failure that belongs on a field.
      if (ApiError.is(error) && error.status === 409) {
        setError("name", { message: `A profile named "${values.name.trim()}" already exists.` })
        return
      }
      setError("root", { message: describeError(error) })
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title={editing ? "Edit profile" : "New profile"}
        description="A profile is what an agent runs as: which CLI, which model, and every prompt it is spawned with."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: `Could not ${editing ? "save" : "create"} the profile`,
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel={editing ? "Save changes" : "Create profile"}
        pending={saving}
      >
        <FieldGroup className="max-h-[60svh] overflow-y-auto px-px py-px">
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="profile-name">Name</FieldLabel>
            <Input
              id="profile-name"
              placeholder="rust-engineer"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={formState.errors.name ? true : undefined}
              {...register("name")}
            />
            {formState.errors.name ? (
              <FieldError errors={[formState.errors.name]} />
            ) : (
              <FieldDescription>
                Unique. Anywhere a profile id is accepted, this name is too.
              </FieldDescription>
            )}
          </Field>

          <div className="grid gap-5 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="profile-role">Role</FieldLabel>
              <FormSelect
                control={control}
                name="role"
                id="profile-role"
                options={ROLES.map((value) => ({ label: ROLE_LABELS[value], value }))}
                disabled={editing}
              />
              <FieldDescription>
                {editing
                  ? "Fixed once the profile exists."
                  : "What this profile is spawned as, and which prompts it owns."}
              </FieldDescription>
            </Field>

            <Field>
              <FieldLabel htmlFor="profile-agent">Agent</FieldLabel>
              <FormSelect
                control={control}
                name="agentKind"
                id="profile-agent"
                options={AGENT_KIND_CHOICES.map((value) => ({
                  label: agentKindChoiceLabel(value),
                  value,
                }))}
              />
              <FieldDescription>
                {agentKind === AUTO_AGENT_KIND
                  ? "The first installed CLI, resolved at spawn time."
                  : "Pinned: spawning fails if this CLI is not installed."}
              </FieldDescription>
            </Field>
          </div>

          <Field>
            {/* No htmlFor: cmdk owns its input's id and names it "Model"
                  itself, through the hidden label its aria-labelledby points at. */}
            <FieldLabel>Model</FieldLabel>
            <div className="flex items-center gap-2">
              <Controller
                control={control}
                name="model"
                render={({ field }) => (
                  <ModelCombobox
                    value={field.value}
                    onChange={field.onChange}
                    agentKind={agentKind}
                    models={models.data}
                  />
                )}
              />
              {model.trim().length > 0 ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setValue("model", "", { shouldDirty: true })}
                >
                  Use default
                </Button>
              ) : null}
            </div>
            <FieldDescription>
              Passed to the agent CLI as-is. Empty means the provider default.
            </FieldDescription>
          </Field>

          <ProfilePromptsField
            key={promptsKey}
            form={form}
            profile={profile}
            stored={stored}
            saved={saved}
            systemIsDefault={systemIsDefault}
            onSystemPromptRestored={(content) => {
              setSystemIsDefault(true)
              if (saved.current.profile) {
                saved.current.profile = { ...saved.current.profile, system_prompt: content }
              }
            }}
          />
        </FieldGroup>
      </FormDialogContent>
    </FormDialog>
  )
}

function agentKindChoiceLabel(choice: AgentKindChoice): string {
  return choice === AUTO_AGENT_KIND ? "Auto-resolve" : agentKindLabel(choice)
}

/**
 * Whether an update would be a no-op.
 *
 * Both bodies come out of `toUpdateRequest`, so their keys are in the same
 * order and a serialisation is a fair comparison.
 */
function sameProfileBody(saved: UpdateProfileRequest | null, next: UpdateProfileRequest): boolean {
  return saved !== null && JSON.stringify(saved) === JSON.stringify(next)
}

/**
 * The body with its system prompt left out when it is the text the daemon last
 * answered with — which the daemon reads as "leave it alone".
 *
 * The box holds the prompt that takes effect, and after a restore that is the
 * role's default. Sending it back with the next change of a name would store
 * the default as this profile's own text and quietly undo the restore.
 */
function withoutUnchangedSystemPrompt(
  body: UpdateProfileRequest,
  saved: UpdateProfileRequest | null,
): UpdateProfileRequest {
  return saved && body.system_prompt === saved.system_prompt
    ? { ...body, system_prompt: undefined }
    : body
}
