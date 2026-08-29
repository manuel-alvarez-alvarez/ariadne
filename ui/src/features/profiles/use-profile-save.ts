/**
 * Saving a profile from its editor: the profile's own fields, then every
 * briefing whose text moved, one request at a time — and the record of what
 * the daemon holds that every one of those writes is measured against.
 *
 * The record is the *baseline*: the profile as form values, as the daemon last
 * confirmed them. It is what "unsaved changes" means (the form differs from
 * it), what Discard puts back, and what each write is diffed against — so a
 * name edited on its own is a `PUT` of the name alone, and a briefing read and
 * left alone is never written back as a text of the profile's own. It moves in
 * pieces: the profile body when its `PUT` lands, each prompt when its own does.
 * That is what makes a second Save after a partial failure send only what is
 * still unsaved.
 *
 * Sequential rather than concurrent so a failure has an unambiguous story:
 * everything named before it is stored, everything after it is not. A failure
 * leaves the form as it was — dirty, with the message on it — and the retry is
 * the same Save button.
 *
 * Two writes happen outside this sequence and are told about: restoring a
 * prompt to its default is written the moment it is asked for (the default is
 * the daemon's text, and the answer is the only way to read what it put back),
 * and a profile updated elsewhere arrives off the event stream. Both hand the
 * hook a new stored state to measure against; neither goes through Save.
 *
 * The form's own notion of dirtiness is deliberately not used: react-hook-form
 * compares against the defaults it was reset with, and those move only when
 * the whole form is reset. The baseline moves a field at a time, so what is
 * unsaved is answered here, from the baseline, and the form is only ever reset
 * *to* it — never to the snapshot a save was made from. The boxes stay open
 * while a save is in flight, and a save takes a while: whatever is typed
 * meanwhile is not in the snapshot, so it is still unsaved when the snapshot
 * lands, and the bar says so.
 */

import { useRef, useState } from "react"
import type { UseFormReturn } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type ProfileDto, type UpdateProfileRequest } from "@/api"
import { describeError } from "@/lib/format"

import {
  changedPrompts,
  type ProfileFormValues,
  type PromptFormValue,
  toUpdateRequest,
} from "./profile-form-values"
import { promptKindLabel } from "./profile-labels"
import { useUpdateProfile, useUpdateProfilePrompt } from "./queries"

interface ProfileSave {
  /** Whether Save would write anything: the form differs from what is stored. */
  dirty: boolean
  /** A save is in flight. */
  saving: boolean
  /** What the daemon is known to hold, as form values: the baseline. */
  stored: ProfileFormValues
  /**
   * Writes what differs from the baseline, in order. Resolves true once
   * everything landed; false when a write failed, with the failure set on the
   * form — on the name for a clash, on `root` for anything else.
   */
  save: (values: ProfileFormValues) => Promise<boolean>
  /** Puts the form back to what is stored. */
  discard: () => void
  /** A new stored state from outside: the form is refilled and measured against it. */
  reseed: (values: ProfileFormValues) => void
  /** One briefing was written past the form (a restore): its box and its baseline follow. */
  promptStored: (prompt: PromptFormValue) => void
  /** The same for the system prompt, which lives on the profile itself. */
  systemPromptStored: (content: string) => void
}

export function useProfileSave(
  profile: ProfileDto,
  form: UseFormReturn<ProfileFormValues>,
  /** What the form was filled with: the stored state as of mounting. */
  initial: ProfileFormValues,
  {
    onProfileSaved,
  }: {
    /** The profile as the daemon answered its `PUT`. */
    onProfileSaved?: (profile: ProfileDto) => void
  } = {},
): ProfileSave {
  const updateProfile = useUpdateProfile()
  const updatePrompt = useUpdateProfilePrompt()
  const { clearErrors, reset, setError, setValue, watch } = form

  // The baseline is state so the answer to "dirty" re-renders with it, and a
  // ref so the save sequence — an async function that moves it several times
  // — always reads the latest rather than the one its render closed over.
  const [baseline, setBaseline] = useState(initial)
  const latest = useRef(initial)
  function moveBaseline(next: ProfileFormValues) {
    latest.current = next
    setBaseline(next)
  }

  const values = watch()
  const dirty = hasUnsavedChanges(values, baseline)

  async function save(values: ProfileFormValues): Promise<boolean> {
    clearErrors("root")
    const body = toUpdateRequest(values)
    const stored = toUpdateRequest(latest.current)
    if (!sameProfileBody(stored, body)) {
      try {
        const updated = await updateProfile.mutateAsync({
          id: profile.id,
          body: withoutUnchangedFields(body, stored),
        })
        moveBaseline({
          ...latest.current,
          name: values.name,
          model: values.model,
          effort: values.effort,
          systemPrompt: values.systemPrompt,
        })
        onProfileSaved?.(updated)
      } catch (error) {
        // A name clash is the one failure that belongs on a field.
        if (ApiError.is(error) && error.status === 409) {
          setError("name", { message: `A profile named "${values.name.trim()}" already exists.` })
          return false
        }
        setError("root", {
          message: `The profile itself could not be saved: ${describeError(error)}`,
        })
        return false
      }
    }

    for (const prompt of changedPrompts(values.prompts, latest.current.prompts)) {
      const label = promptKindLabel(prompt.kind).toLowerCase()
      try {
        await updatePrompt.mutateAsync({ id: profile.id, ...prompt })
        moveBaseline({
          ...latest.current,
          prompts: replacePrompt(latest.current.prompts, prompt),
        })
      } catch (error) {
        setError("root", {
          message: `The ${label} could not be saved: ${describeError(error)} Everything saved before it is already stored; saving again retries only what is left.`,
        })
        return false
      }
    }

    // Everything landed, and the baseline says so. The form itself is left
    // alone on purpose: what was typed while the writes were in flight is
    // still in the boxes, differs from the baseline, and is therefore the
    // next thing to save — resetting to the snapshot here would erase it.
    toast.success("Profile updated", { description: values.name.trim() })
    return true
  }

  function discard() {
    reset(latest.current)
  }

  function reseed(values: ProfileFormValues) {
    moveBaseline(values)
    reset(values)
  }

  function promptStored(prompt: PromptFormValue) {
    const index = latest.current.prompts.findIndex((one) => one.kind === prompt.kind)
    if (index >= 0) setValue(`prompts.${index}.content`, prompt.content)
    moveBaseline({ ...latest.current, prompts: replacePrompt(latest.current.prompts, prompt) })
  }

  function systemPromptStored(content: string) {
    setValue("systemPrompt", content)
    moveBaseline({ ...latest.current, systemPrompt: content })
  }

  return {
    dirty,
    saving: updateProfile.isPending || updatePrompt.isPending,
    stored: baseline,
    save,
    discard,
    reseed,
    promptStored,
    systemPromptStored,
  }
}

/**
 * Whether saving would write anything, which is the only sense in which the
 * editor is dirty: the profile body as it would go on the wire differs from
 * the stored one, or some briefing's text moved.
 */
function hasUnsavedChanges(values: ProfileFormValues, baseline: ProfileFormValues): boolean {
  return (
    !sameProfileBody(toUpdateRequest(baseline), toUpdateRequest(values)) ||
    changedPrompts(values.prompts, baseline.prompts).length > 0
  )
}

/** The stored prompts with one kind's text replaced by what just landed. */
function replacePrompt(
  prompts: readonly PromptFormValue[],
  saved: PromptFormValue,
): PromptFormValue[] {
  return prompts.some((prompt) => prompt.kind === saved.kind)
    ? prompts.map((prompt) => (prompt.kind === saved.kind ? saved : prompt))
    : [...prompts, saved]
}

/**
 * Whether an update would be a no-op.
 *
 * Both bodies come out of `toUpdateRequest`, so their keys are in the same
 * order and a serialisation is a fair comparison.
 */
function sameProfileBody(saved: UpdateProfileRequest, next: UpdateProfileRequest): boolean {
  return JSON.stringify(saved) === JSON.stringify(next)
}

/**
 * The body with every field that still holds what was last stored left out —
 * which the daemon reads as "leave it alone".
 *
 * `UpdateProfileRequest` is a partial update, and both of these fields are
 * boxes that always hold *something*: an untouched one would otherwise be
 * written back on every save.
 *
 * For the model and the effort that is a stale overwrite waiting to happen —
 * each box holds a value, or the empty string `profile-form-values.ts` turns
 * into the "back on auto" sentinel, so a name edited on its own would re-pin
 * the profile to whatever the boxes were seeded with and undo a model changed
 * from the CLI or another window meanwhile.
 *
 * For the system prompt it is sharper still: the box holds the prompt that
 * takes effect, and after a restore that is the role's default. Sending it back
 * with the next change of a name would store the default as this profile's own
 * text and quietly undo the restore.
 */
function withoutUnchangedFields(
  body: UpdateProfileRequest,
  saved: UpdateProfileRequest,
): UpdateProfileRequest {
  const model = body.model === saved.model ? undefined : body.model
  return {
    ...body,
    model,
    // An effort belongs to the model it is run at, and the daemon drops it from
    // a pin whose model moves — so a model on the wire takes the effort box
    // with it whether that box changed or not.
    effort: model === undefined && body.effort === saved.effort ? undefined : body.effort,
    system_prompt: body.system_prompt === saved.system_prompt ? undefined : body.system_prompt,
  }
}
