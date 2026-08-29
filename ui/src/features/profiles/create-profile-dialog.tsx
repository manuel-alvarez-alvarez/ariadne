/**
 * `ariadne profile create`, as a small form: a name, a role, and what the
 * profile runs on.
 *
 * Nothing more, on purpose. A profile that does not exist yet owns no prompts
 * to read — the defaults are the daemon's text with no endpoint handing them
 * out — so the new profile is created on its role's own prompts and lands in
 * the editor (see `profiles-page.tsx`), which is where every one of them is
 * edited, the system prompt included.
 *
 * Validation is client-side for the two required fields and daemon-side for
 * everything it alone can know: a duplicate name comes back as a 409 and is
 * shown on the name field, anything else as an alert above the buttons.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type ProfileDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { describeError, ROLE_LABELS } from "@/lib/format"

import { PinPicker } from "./pin-picker"
import {
  emptyProfileFormValues,
  type ProfileFormValues,
  profileFormSchema,
  toCreateRequest,
} from "./profile-form-values"
import { ROLES } from "./profile-labels"
import { modelsQueryOptions, useCreateProfile } from "./queries"

const EMPTY = emptyProfileFormValues()

export function CreateProfileDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** What the screen does with the new profile once the dialog has closed. */
  onCreated?: (profile: ProfileDto) => void
}) {
  const createProfile = useCreateProfile()

  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: EMPTY,
  })
  const { control, formState, handleSubmit, register, setError, setValue, watch } = form
  useResetOnOpen(open, form, EMPTY, createProfile)

  const effort = watch("effort")

  // The catalog behind the pin picker. Only asked for while the dialog is up,
  // and allowed to fail: an undefined catalog leaves the field free-text.
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })

  async function submit(values: ProfileFormValues) {
    try {
      const created = await createProfile.mutateAsync(toCreateRequest(values))
      toast.success("Profile created", { description: created.name })
      onOpenChange(false)
      onCreated?.(created)
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
        title="New profile"
        description="A profile is what an agent runs as: a role, and the CLI and model that run it. Its prompts are edited once it exists."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: "Could not create the profile",
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel="Create profile"
        pending={createProfile.isPending}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="new-profile-name">Name</FieldLabel>
            <Input
              id="new-profile-name"
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

          <Field>
            <FieldLabel htmlFor="new-profile-role">Role</FieldLabel>
            <FormSelect
              control={control}
              name="role"
              id="new-profile-role"
              options={ROLES.map((value) => ({ label: ROLE_LABELS[value], value }))}
            />
            <FieldDescription>
              What this profile is spawned as, and which prompts it owns. Fixed once it exists.
            </FieldDescription>
          </Field>

          <Field data-invalid={formState.errors.model ? true : undefined}>
            <FieldLabel htmlFor="new-profile-pin">Runs on</FieldLabel>
            <Controller
              control={control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="new-profile-pin"
                  label="Runs on"
                  model={field.value}
                  effort={effort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models.data}
                  invalid={formState.errors.model ? true : undefined}
                  // Nothing stands behind a profile the way a profile stands
                  // behind a task's slots: its empty is auto and nothing else.
                  unpinnedLabel="auto — first installed CLI, on its own default model"
                />
              )}
            />
            {formState.errors.model ? (
              <FieldError errors={[formState.errors.model]} />
            ) : (
              <FieldDescription>
                The agent CLI and, after a <code>:</code>, the model of it, with the effort that
                model is run at. Empty is auto: the first installed CLI, on its own default model.
              </FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
