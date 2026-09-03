import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon, Trash2Icon, Undo2Icon } from "lucide-react"
import { type RefObject, useCallback, useEffect, useRef, useState } from "react"
import { type Control, Controller, useForm } from "react-hook-form"
import { type Location, useBlocker } from "react-router-dom"

import type { ModelDto, ProfileDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { ErrorState } from "@/components/error-state"
import { submitOnChord } from "@/components/form-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { When } from "@/components/when"
import { cn, describeError } from "@/lib/format"
import { PROFILE_PARAM } from "@/routes/paths"

import { DeleteProfileDialog } from "./delete-profile-dialog"
import { PinPicker } from "./pin-picker"
import {
  type ProfileFormValues,
  profileFormSchema,
  profileToFormValues,
} from "./profile-form-values"
import { roleLabel } from "./profile-labels"
import { modelsQueryOptions, useResetSystemPrompt } from "./queries"
import { useProfileSave } from "./use-profile-save"

const UNPINNED_LABEL = "auto — first installed CLI, on its own default model"

export function ProfileEditor({
  profile,
  onBack,
  onDeleted,
}: {
  profile: ProfileDto
  onBack: () => void
  onDeleted: () => void
}) {
  const [deleteOpen, setDeleteOpen] = useState(false)
  const leaving = useRef(false)
  const models = useQuery(modelsQueryOptions())

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1">
        <Button
          variant="ghost"
          size="icon-sm"
          className="-ml-1.5 md:hidden"
          aria-label="Back to the list"
          onClick={onBack}
        >
          <ArrowLeftIcon />
        </Button>
        <h2 className="min-w-0 truncate font-heading text-base font-semibold">{profile.name}</h2>
        <Badge variant="secondary">{roleLabel(profile.role)}</Badge>
        <p className="text-xs text-muted-foreground">
          Created <When at={profile.created_at} label="created" /> · updated{" "}
          <When at={profile.updated_at} label="updated" />
        </p>
        <Button variant="outline" size="sm" className="ml-auto" onClick={() => setDeleteOpen(true)}>
          <Trash2Icon />
          Delete
        </Button>
      </header>
      <ProfileForm profile={profile} models={models.data} leaving={leaving} />
      <DeleteProfileDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        profile={profile}
        onDeleted={() => {
          leaving.current = true
          onDeleted()
        }}
      />
    </div>
  )
}

function leavesProfile(current: Location, next: Location): boolean {
  return (
    current.pathname !== next.pathname ||
    new URLSearchParams(current.search).get(PROFILE_PARAM) !==
      new URLSearchParams(next.search).get(PROFILE_PARAM)
  )
}

function ProfileForm({
  profile,
  models,
  leaving,
}: {
  profile: ProfileDto
  models: ModelDto[] | undefined
  leaving: RefObject<boolean>
}) {
  const [initial] = useState(() => profileToFormValues(profile))
  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: initial,
  })
  const { control, formState, handleSubmit, register, setError, setValue, watch } = form
  const [systemIsDefault, setSystemIsDefault] = useState(profile.system_prompt_is_default)
  useEffect(
    () => setSystemIsDefault(profile.system_prompt_is_default),
    [profile.system_prompt_is_default],
  )
  const save = useProfileSave(profile, form, initial, {
    onProfileSaved: (updated) => setSystemIsDefault(updated.system_prompt_is_default),
  })
  const latest = useRef({ dirty: save.dirty, reseed: save.reseed })
  latest.current = { dirty: save.dirty, reseed: save.reseed }
  const seededFrom = useRef(profile)
  useEffect(() => {
    if (seededFrom.current === profile) return
    seededFrom.current = profile
    if (!latest.current.dirty) latest.current.reseed(profileToFormValues(profile))
  }, [profile])
  const blocker = useBlocker(
    useCallback(
      ({ currentLocation, nextLocation }: { currentLocation: Location; nextLocation: Location }) =>
        save.dirty && !leaving.current && leavesProfile(currentLocation, nextLocation),
      [save.dirty, leaving],
    ),
  )
  const resetSystemPrompt = useResetSystemPrompt()
  const restoring = resetSystemPrompt.isPending
  async function restoreSystemPrompt() {
    try {
      const updated = await resetSystemPrompt.mutateAsync(profile.id)
      save.systemPromptStored(updated.system_prompt)
      setSystemIsDefault(true)
    } catch (error) {
      setError("root", {
        message: `The system prompt could not be restored: ${describeError(error)}`,
      })
    }
  }
  const effort = watch("effort")
  const model = watch("model")
  const catalogModel = models?.find((entry) => entry.id === model.trim())

  return (
    <form
      onSubmit={handleSubmit((values) => {
        if (!restoring) void save.save(values)
      })}
      onKeyDown={save.saving || restoring ? undefined : submitOnChord}
      className="flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto contain-paint"
      aria-label={`Edit ${profile.name}`}
    >
      <FieldGroup className="px-px pb-4">
        <div className="flex flex-col gap-5 @md/field-group:flex-row @md/field-group:gap-4">
          <Field
            className="@md/field-group:w-72"
            data-invalid={formState.errors.name ? true : undefined}
          >
            <FieldLabel htmlFor="profile-name">Name</FieldLabel>
            <Input
              id="profile-name"
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
          <Field
            className="@md/field-group:flex-1"
            data-invalid={formState.errors.model ? true : undefined}
          >
            <FieldLabel htmlFor="profile-pin">Runs on</FieldLabel>
            <Controller
              control={control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="profile-pin"
                  label="Runs on"
                  model={field.value}
                  effort={effort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models}
                  invalid={formState.errors.model ? true : undefined}
                  unpinnedLabel={UNPINNED_LABEL}
                />
              )}
            />
            {formState.errors.model ? (
              <FieldError errors={[formState.errors.model]} />
            ) : (
              <FieldDescription>
                {catalogModel?.description ??
                  "The agent CLI and, after a “:”, the model of it, with the effort that model is run at. Empty is auto: the first installed CLI, on its own default model."}
              </FieldDescription>
            )}
          </Field>
        </div>
        <SystemPromptEditor
          control={control}
          isDefault={systemIsDefault}
          restoring={restoring}
          saving={save.saving}
          onRestore={() => void restoreSystemPrompt()}
        />
      </FieldGroup>
      {save.dirty ? (
        <div className="sticky bottom-0 mt-auto flex flex-col gap-2 border-t bg-background pt-3">
          {formState.errors.root ? (
            <ErrorState
              title="Could not save the profile"
              error={null}
              description={formState.errors.root.message}
            />
          ) : null}
          <div className="flex items-center gap-2">
            <p className="text-sm text-muted-foreground">
              {save.saving ? "Saving…" : restoring ? "Restoring…" : "Unsaved changes"}
            </p>
            <Button
              type="button"
              variant="outline"
              className="ml-auto"
              disabled={save.saving || restoring}
              onClick={save.discard}
            >
              Discard
            </Button>
            <Button type="submit" pending={save.saving} disabled={restoring}>
              Save
            </Button>
          </div>
        </div>
      ) : null}
      <ConfirmDialog
        open={blocker.state === "blocked"}
        onClose={() => blocker.reset?.()}
        title="Discard changes?"
        description="This profile has unsaved changes. Leaving it now drops them."
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => blocker.proceed?.()}
      />
    </form>
  )
}

function SystemPromptEditor({
  control,
  isDefault,
  restoring,
  saving,
  onRestore,
}: {
  control: Control<ProfileFormValues>
  isDefault: boolean
  restoring: boolean
  saving: boolean
  onRestore: () => void
}) {
  const [confirming, setConfirming] = useState(false)
  return (
    <Field>
      <div className="flex items-center gap-2">
        <FieldLabel htmlFor="system-prompt">System prompt</FieldLabel>
        <Badge variant="outline">{isDefault ? "default" : "edited"}</Badge>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ml-auto"
          aria-label="Restore System prompt to default"
          disabled={isDefault || restoring || saving}
          onClick={() => setConfirming(true)}
        >
          <Undo2Icon />
          Restore default
        </Button>
      </div>
      <Controller
        control={control}
        name="systemPrompt"
        render={({ field }) => (
          <Textarea
            id="system-prompt"
            aria-label="System prompt"
            spellCheck={false}
            className={cn("min-h-96 resize-y font-mono text-xs leading-relaxed")}
            {...field}
          />
        )}
      />
      <FieldDescription className="text-xs">
        Prepended to whatever Ariadne tells the agent about its task.
      </FieldDescription>
      <ConfirmDialog
        open={confirming}
        onClose={() => setConfirming(false)}
        title="Restore system prompt to its default?"
        description="The text this profile has of its own is dropped and the system prompt goes back to the default of its role. It is written straight away: discarding the other edits afterwards does not put it back."
        confirmLabel="Restore default"
        destructive
        onConfirm={() => {
          setConfirming(false)
          onRestore()
        }}
      />
    </Field>
  )
}
