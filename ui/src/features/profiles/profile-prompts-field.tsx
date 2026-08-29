/**
 * Every prompt a profile is spawned with, as one folded stack inside the
 * profile form.
 *
 * The system prompt leads, because it is the one every profile has and the one
 * most often rewritten; the role's briefings follow, filled with what the
 * profile is briefed with — its own text where it has one, the default of the
 * kind where it has none, each said to be which.
 *
 * Restoring a default is the one thing written the moment it is asked for: the
 * default is the daemon's text and no form holds a copy of it, so the answer is
 * both the write and the only way to read what it put back. The baseline moves
 * with it, so the submit that follows does not write the default straight back
 * as a text of the profile's own.
 */

import type { UseQueryResult } from "@tanstack/react-query"
import { type RefObject, useState } from "react"
import type { UseFormReturn } from "react-hook-form"

import type { ProfileDto, ProfilePromptDto, PromptKind, UpdateProfileRequest } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Field, FieldDescription, FieldTitle } from "@/components/ui/field"
import { describeError } from "@/lib/format"

import { LoadingPrompts } from "./loading-prompts"
import type { ProfileFormValues, PromptFormValue } from "./profile-form-values"
import { PROMPT_KIND_HINTS, promptKindLabel, roleLabel } from "./profile-labels"
import { PromptFormField } from "./prompt-form-field"
import { useResetProfilePrompt, useResetSystemPrompt } from "./queries"

/** The section key of the system prompt, which is not one of the kinds. */
const SYSTEM_PROMPT_SECTION = "system_prompt"

/**
 * What the daemon is known to hold, as the dialog last saw it.
 *
 * Both halves move: the profile body is replaced when its `PUT` lands, each
 * prompt when its own does. That is what makes a second submit after a partial
 * failure send only what is still unsaved.
 */
export interface SavedState {
  /** The last body the daemon accepted, or null while creating. */
  profile: UpdateProfileRequest | null
  prompts: PromptFormValue[]
}

/** The saved prompts with one kind's text replaced by what just landed. */
export function replacePrompt(
  prompts: PromptFormValue[],
  saved: PromptFormValue,
): PromptFormValue[] {
  return prompts.some((prompt) => prompt.kind === saved.kind)
    ? prompts.map((prompt) => (prompt.kind === saved.kind ? saved : prompt))
    : [...prompts, saved]
}

export function ProfilePromptsField({
  form,
  profile,
  stored,
  saved,
  systemIsDefault,
  onSystemPromptRestored,
}: {
  form: UseFormReturn<ProfileFormValues>
  /** The profile being edited, or null while creating. */
  profile: ProfileDto | null
  /** What this profile is briefed with; only asked for when editing. */
  stored: UseQueryResult<ProfilePromptDto[]>
  /** The dialog's baseline, which a restore moves. */
  saved: RefObject<SavedState>
  systemIsDefault: boolean
  onSystemPromptRestored: (content: string) => void
}) {
  const editing = profile !== null
  const { setError, setValue, watch } = form
  const resetPrompt = useResetProfilePrompt()
  const resetSystemPrompt = useResetSystemPrompt()
  const role = watch("role")
  const systemPrompt = watch("systemPrompt")
  const prompts = watch("prompts")
  const storedData = stored.data

  /** Which prompt sections are folded open, by kind. */
  const [openSections, setOpenSections] = useState<Record<string, boolean>>({})

  function sectionOpen(key: string, fallback: boolean): boolean {
    return openSections[key] ?? fallback
  }

  function toggleSection(key: string, next: boolean): void {
    setOpenSections((current) => ({ ...current, [key]: next }))
  }

  /** Whether the profile has no text of its own for one briefing. */
  function isDefault(kind: PromptKind): boolean {
    return storedData?.find((prompt) => prompt.kind === kind)?.is_default ?? true
  }

  /**
   * Drop the text set on one prompt and fill its editor with the default that
   * takes over — a write of its own, since the default is the daemon's to say.
   *
   * The baseline moves with it, so the submit that follows does not write the
   * default straight back as a text of the profile's own.
   */
  async function restore(kind: PromptKind, index: number) {
    if (!profile) return
    try {
      const prompt = await resetPrompt.mutateAsync({ id: profile.id, kind })
      setValue(`prompts.${index}.content`, prompt.content)
      saved.current.prompts = replacePrompt(saved.current.prompts, {
        kind,
        content: prompt.content,
      })
    } catch (error) {
      setError("root", {
        message: `The ${promptKindLabel(kind).toLowerCase()} could not be restored: ${describeError(error)}`,
      })
    }
  }

  /** The same for the system prompt, which lives on the profile itself. */
  async function restoreSystemPrompt() {
    if (!profile) return
    try {
      const updated = await resetSystemPrompt.mutateAsync(profile.id)
      setValue("systemPrompt", updated.system_prompt)
      onSystemPromptRestored(updated.system_prompt)
    } catch (error) {
      setError("root", {
        message: `The system prompt could not be restored: ${describeError(error)}`,
      })
    }
  }
  return (
    <Field>
      {/* A heading rather than a label: what follows is a stack of
                sections, not one control to point a `for` at. */}
      <FieldTitle>Prompts</FieldTitle>
      <FieldDescription>
        {editing
          ? `What a ${roleLabel(role).toLowerCase()} is spawned with — its own text where it has one, its role's default where it has none. Saved with the rest of the form; restoring a default is written straight away.`
          : `What a ${roleLabel(role).toLowerCase()} is spawned with. Leave the system prompt blank to run on the role's own, and edit the briefings once the profile exists.`}
      </FieldDescription>

      <div className="flex flex-col gap-3">
        <PromptFormField
          label="System prompt"
          hint="Prepended to whatever Ariadne tells the agent about its task."
          value={systemPrompt}
          onChange={(content) =>
            setValue("systemPrompt", content, { shouldDirty: true, shouldValidate: true })
          }
          // Creating, a blank box is the role's default; editing, the
          // daemon says whether the text in it is one.
          isDefault={editing ? systemIsDefault : systemPrompt.trim().length === 0}
          onReset={editing ? () => void restoreSystemPrompt() : undefined}
          resetting={resetSystemPrompt.isPending}
          // The one prompt every profile has, and the one most often
          // rewritten: open unless it is deliberately folded away.
          open={sectionOpen(SYSTEM_PROMPT_SECTION, true)}
          onOpenChange={(next) => toggleSection(SYSTEM_PROMPT_SECTION, next)}
          placeholder={
            editing
              ? undefined
              : "Blank: the role's own. Or: You are a Rust engineer working inside a dedicated git worktree…"
          }
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
            isDefault={isDefault(prompt.kind)}
            onReset={() => void restore(prompt.kind, index)}
            resetting={resetPrompt.isPending}
            open={sectionOpen(prompt.kind, false)}
            onOpenChange={(next) => toggleSection(prompt.kind, next)}
          />
        ))}

        {editing && stored.isPending ? <LoadingPrompts folded /> : null}

        {editing && stored.isError ? (
          <ErrorState
            title="Could not load the briefings"
            error={stored.error}
            onRetry={() => void stored.refetch()}
          />
        ) : null}
      </div>
    </Field>
  )
}
