/**
 * Create and edit dialog for a profile — one form for both, because the two
 * differ only in where they post and in the one field the daemon cannot change
 * after creation (the role).
 *
 * Every prompt the profile is spawned with is edited here, not just the system
 * one: the role's briefings are folded into the same form, prefilled from the
 * role defaults when creating and from the profile's own prompts when editing.
 * Nothing is written until the form is submitted, which is the one thing that
 * separates these editors from the details panel's — there, each box saves
 * itself.
 *
 * How they are written differs by mode. Create sends the whole profile in one
 * request, briefings included, so only the ones edited away from their default
 * go into the body. Update has no room for them: the profile's own fields go in
 * its `PUT`, and every changed briefing is a `PUT` of its own afterwards. Those
 * are separate requests that can fail separately, so a failure leaves the
 * dialog open, says which write failed, and remembers what already landed so a
 * retry does not repeat it.
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

import {
  ApiError,
  type ProfileDto,
  type PromptKind,
  type Role,
  type UpdateProfileRequest,
} from "@/api"
import { ErrorState } from "@/components/error-state"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { describeError } from "@/lib/errors"
import { ROLE_LABELS } from "@/lib/labels"

import { ModelCombobox } from "./model-combobox"
import {
  AGENT_KIND_CHOICES,
  type AgentKindChoice,
  AUTO_AGENT_KIND,
  changedPrompts,
  emptyProfileFormValues,
  type ProfileFormValues,
  type PromptFormValue,
  profileFormSchema,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"
import {
  agentKindLabel,
  PROMPT_KIND_HINTS,
  promptKindLabel,
  ROLES,
  roleLabel,
} from "./profile-labels"
import { PromptFormField } from "./prompt-form-field"
import {
  modelsQueryOptions,
  profilePromptsQueryOptions,
  rolePromptDefaultsQueryOptions,
  useCreateProfile,
  useUpdateProfile,
  useUpdateProfilePrompt,
} from "./queries"

/** The section key of the system prompt, which is not one of the kinds. */
const SYSTEM_PROMPT_SECTION = "system_prompt"

/**
 * What the daemon is known to hold, as the dialog last saw it.
 *
 * Both halves move: the profile body is replaced when its `PUT` lands, each
 * prompt when its own does. That is what makes a second submit after a partial
 * failure send only what is still unsaved.
 */
interface SavedState {
  /** The last body the daemon accepted, or null while creating. */
  profile: UpdateProfileRequest | null
  prompts: PromptFormValue[]
}

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
  const updatePrompt = useUpdateProfilePrompt(profile?.id ?? "")
  const saving = createProfile.isPending || updateProfile.isPending || updatePrompt.isPending

  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: emptyProfileFormValues(),
  })
  const { control, formState, handleSubmit, register, reset, setError, setValue, watch } = form

  /** Which prompt sections are folded open, by kind. */
  const [openSections, setOpenSections] = useState<Record<string, boolean>>({})
  const saved = useRef<SavedState>({ profile: null, prompts: [] })
  /** What the form was last reset for, so a re-render does not undo edits. */
  const openedFor = useRef<string | null>(null)
  /** What the prompt editors were last filled from: a role, or the profile. */
  const seededFrom = useRef<string | null>(null)

  const role = watch("role")
  const model = watch("model")
  const agentKind = watch("agentKind")
  const systemPrompt = watch("systemPrompt")
  const prompts = watch("prompts")

  // The catalog behind the model combobox. Only asked for while the dialog is
  // up, and allowed to fail: an undefined catalog leaves the field free-text.
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })

  // The role's built-in prompts. Needed in both modes: to prefill a new
  // profile's editors, and to answer "restore default" in either.
  const defaults = useQuery({ ...rolePromptDefaultsQueryOptions(role), enabled: open })

  // What this profile actually holds, which is only a question when editing.
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
    setOpenSections({})
    saved.current = { profile: profile ? toUpdateRequest(values) : null, prompts: [] }
  }, [open, profile, reset])

  // Creating: the editors are the role's defaults, and switching role swaps
  // them wholesale. Whatever was typed into the old role's briefings goes with
  // them — they are not prompts this profile would have had.
  const defaultsData = defaults.data
  useEffect(() => {
    if (!open || editing || !defaultsData || seededFrom.current === defaultsData.role) return
    seededFrom.current = defaultsData.role
    setValue("systemPrompt", defaultsData.system_prompt)
    setValue(
      "prompts",
      defaultsData.prompts.map((prompt) => ({ kind: prompt.kind, content: prompt.content })),
    )
  }, [open, editing, defaultsData, setValue])

  // Editing: the briefings are the profile's own, and they are also the
  // baseline every later submit is diffed against.
  const storedData = stored.data
  useEffect(() => {
    if (!open || !editing || !storedData || seededFrom.current === "stored") return
    seededFrom.current = "stored"
    const values = storedData.map((prompt) => ({ kind: prompt.kind, content: prompt.content }))
    setValue("prompts", values)
    saved.current.prompts = values
  }, [open, editing, storedData, setValue])

  /** The role default of one prompt, or undefined while they are unknown. */
  function defaultContent(kind: PromptKind): string | undefined {
    return defaultsData?.prompts.find((prompt) => prompt.kind === kind)?.content
  }

  function sectionOpen(key: string, fallback: boolean): boolean {
    return openSections[key] ?? fallback
  }

  function toggleSection(key: string, next: boolean): void {
    setOpenSections((current) => ({ ...current, [key]: next }))
  }

  async function create(values: ProfileFormValues) {
    const seeded = defaultsData?.prompts ?? []
    const created = await createProfile.mutateAsync(toCreateRequest(values, seeded))
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
        await updateProfile.mutateAsync({ id: target.id, body })
        saved.current.profile = body
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
        await updatePrompt.mutateAsync(prompt)
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <form onSubmit={handleSubmit(submit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit profile" : "New profile"}</DialogTitle>
            <DialogDescription>
              A profile is what an agent runs as: which CLI, which model, and every prompt it is
              spawned with.
            </DialogDescription>
          </DialogHeader>

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
              <Controller
                control={control}
                name="role"
                render={({ field }) => (
                  <Field>
                    <FieldLabel htmlFor="profile-role">Role</FieldLabel>
                    <Select
                      value={field.value}
                      onValueChange={(value) => field.onChange(value)}
                      disabled={editing}
                    >
                      <SelectTrigger id="profile-role" className="w-full">
                        <SelectValue>{(value: Role) => ROLE_LABELS[value]}</SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {ROLES.map((role) => (
                          <SelectItem key={role} value={role}>
                            {ROLE_LABELS[role]}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      {editing
                        ? "Fixed once the profile exists."
                        : "What this profile is spawned as, and which prompts it owns."}
                    </FieldDescription>
                  </Field>
                )}
              />

              <Controller
                control={control}
                name="agentKind"
                render={({ field }) => (
                  <Field>
                    <FieldLabel htmlFor="profile-agent">Agent</FieldLabel>
                    <Select value={field.value} onValueChange={(value) => field.onChange(value)}>
                      <SelectTrigger id="profile-agent" className="w-full">
                        <SelectValue>
                          {(value: AgentKindChoice) => agentKindChoiceLabel(value)}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {AGENT_KIND_CHOICES.map((choice) => (
                          <SelectItem key={choice} value={choice}>
                            {agentKindChoiceLabel(choice)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      {field.value === AUTO_AGENT_KIND
                        ? "The first installed CLI, resolved at spawn time."
                        : "Pinned: spawning fails if this CLI is not installed."}
                    </FieldDescription>
                  </Field>
                )}
              />
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

            <Field>
              {/* A heading rather than a label: what follows is a stack of
                  sections, not one control to point a `for` at. */}
              <FieldTitle>Prompts</FieldTitle>
              <FieldDescription>
                What a {roleLabel(role).toLowerCase()} is spawned with. Saved with the rest of the
                form — nothing here is written until then.
              </FieldDescription>

              <div className="flex flex-col gap-3">
                <PromptFormField
                  label="System prompt"
                  hint="Prepended to whatever Ariadne tells the agent about its task."
                  value={systemPrompt}
                  onChange={(content) =>
                    setValue("systemPrompt", content, { shouldDirty: true, shouldValidate: true })
                  }
                  defaultContent={defaultsData?.system_prompt}
                  // The one prompt every profile has, and the one most often
                  // rewritten: open unless it is deliberately folded away, and
                  // forced open again by a validation error it would hide.
                  open={
                    sectionOpen(SYSTEM_PROMPT_SECTION, true) ||
                    Boolean(formState.errors.systemPrompt)
                  }
                  onOpenChange={(next) => toggleSection(SYSTEM_PROMPT_SECTION, next)}
                  error={formState.errors.systemPrompt?.message}
                  placeholder="You are a Rust engineer working inside a dedicated git worktree…"
                />

                {prompts.map((prompt, index) => (
                  <PromptFormField
                    key={prompt.kind}
                    label={promptKindLabel(prompt.kind)}
                    hint={PROMPT_KIND_HINTS[prompt.kind]}
                    value={prompt.content}
                    onChange={(content) =>
                      setValue(`prompts.${index}.content`, content, { shouldDirty: true })
                    }
                    defaultContent={defaultContent(prompt.kind)}
                    open={sectionOpen(prompt.kind, false)}
                    onOpenChange={(next) => toggleSection(prompt.kind, next)}
                  />
                ))}

                {(editing ? stored.isPending : defaults.isPending) ? <LoadingPrompts /> : null}

                {editing && stored.isError ? (
                  <ErrorState
                    title="Could not load the briefings"
                    error={stored.error}
                    onRetry={() => void stored.refetch()}
                  />
                ) : null}

                {defaults.isError ? (
                  <ErrorState
                    title="Could not load the role's default prompts"
                    error={defaults.error}
                    onRetry={() => void defaults.refetch()}
                  />
                ) : null}
              </div>
            </Field>
          </FieldGroup>

          {formState.errors.root ? (
            <Alert variant="destructive">
              <AlertTitle>{editing ? "Could not save" : "Could not create"} the profile</AlertTitle>
              <AlertDescription>{formState.errors.root.message}</AlertDescription>
            </Alert>
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" disabled={saving} />}>
              Cancel
            </DialogClose>
            <Button type="submit" pending={saving}>
              {editing ? "Save changes" : "Create profile"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Standing in for prompts whose count is not known until they arrive. */
function LoadingPrompts() {
  return (
    <div className="flex flex-col gap-2">
      <Skeleton className="h-9 w-full" />
      <Skeleton className="h-9 w-full" />
    </div>
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

/** The saved prompts with one kind's text replaced by what just landed. */
function replacePrompt(prompts: PromptFormValue[], saved: PromptFormValue): PromptFormValue[] {
  return prompts.some((prompt) => prompt.kind === saved.kind)
    ? prompts.map((prompt) => (prompt.kind === saved.kind ? saved : prompt))
    : [...prompts, saved]
}
