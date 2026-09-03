import { useRef, useState } from "react"
import type { UseFormReturn } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type ProfileDto, type UpdateProfileRequest } from "@/api"
import { describeError } from "@/lib/format"

import { type ProfileFormValues, toUpdateRequest } from "./profile-form-values"
import { useUpdateProfile } from "./queries"

interface ProfileSave {
  dirty: boolean
  saving: boolean
  save: (values: ProfileFormValues) => Promise<boolean>
  discard: () => void
  reseed: (values: ProfileFormValues) => void
  systemPromptStored: (content: string) => void
}

export function useProfileSave(
  profile: ProfileDto,
  form: UseFormReturn<ProfileFormValues>,
  initial: ProfileFormValues,
  { onProfileSaved }: { onProfileSaved?: (profile: ProfileDto) => void } = {},
): ProfileSave {
  const updateProfile = useUpdateProfile()
  const { clearErrors, reset, setError, setValue, watch } = form
  const [baseline, setBaseline] = useState(initial)
  const latest = useRef(initial)
  function moveBaseline(next: ProfileFormValues) {
    latest.current = next
    setBaseline(next)
  }
  const values = watch()
  const dirty = !sameProfileBody(toUpdateRequest(baseline), toUpdateRequest(values))

  async function save(values: ProfileFormValues): Promise<boolean> {
    clearErrors("root")
    const body = toUpdateRequest(values)
    const stored = toUpdateRequest(latest.current)
    if (sameProfileBody(stored, body)) return true
    try {
      const updated = await updateProfile.mutateAsync({
        id: profile.id,
        body: withoutUnchangedFields(body, stored),
      })
      moveBaseline(values)
      onProfileSaved?.(updated)
      toast.success("Profile updated", { description: values.name.trim() })
      return true
    } catch (error) {
      if (ApiError.is(error) && error.status === 409) {
        setError("name", { message: `A profile named "${values.name.trim()}" already exists.` })
      } else {
        setError("root", { message: `The profile could not be saved: ${describeError(error)}` })
      }
      return false
    }
  }

  return {
    dirty,
    saving: updateProfile.isPending,
    save,
    discard: () => reset(latest.current),
    reseed: (values) => {
      moveBaseline(values)
      reset(values)
    },
    systemPromptStored: (content) => {
      setValue("systemPrompt", content)
      moveBaseline({ ...latest.current, systemPrompt: content })
    },
  }
}

function sameProfileBody(saved: UpdateProfileRequest, next: UpdateProfileRequest): boolean {
  return JSON.stringify(saved) === JSON.stringify(next)
}

function withoutUnchangedFields(
  body: UpdateProfileRequest,
  saved: UpdateProfileRequest,
): UpdateProfileRequest {
  const model = body.model === saved.model ? undefined : body.model
  return {
    ...body,
    model,
    effort: model === undefined && body.effort === saved.effort ? undefined : body.effort,
    system_prompt: body.system_prompt === saved.system_prompt ? undefined : body.system_prompt,
  }
}
