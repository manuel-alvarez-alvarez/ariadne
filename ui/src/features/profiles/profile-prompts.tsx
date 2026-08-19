/**
 * Every prompt a profile is spawned with, in one stack of editors.
 *
 * Which prompts those are is not decided here: `GET /v1/profiles/{id}/prompts`
 * answers exactly the kinds the profile's role owns, in briefing order, so a
 * planner shows one briefing and an engineer three without this file knowing
 * which. Only the system prompt is added on top — it lives on the profile
 * itself rather than in that list.
 *
 * The list is fetched when the details panel opens, which is the only time it
 * is on screen.
 */

import { useQuery } from "@tanstack/react-query"

import type { ProfileDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Skeleton } from "@/components/ui/skeleton"

import { PROMPT_KIND_HINTS, PROMPT_KIND_LABELS, roleLabel } from "./profile-labels"
import { PromptEditor } from "./prompt-editor"
import {
  profilePromptsQueryOptions,
  useResetProfilePrompt,
  useResetSystemPrompt,
  useUpdateProfile,
  useUpdateProfilePrompt,
} from "./queries"

export function ProfilePrompts({ profile }: { profile: ProfileDto }) {
  const prompts = useQuery(profilePromptsQueryOptions(profile.id))
  const updateProfile = useUpdateProfile()
  const resetSystemPrompt = useResetSystemPrompt(profile.id)
  const updatePrompt = useUpdateProfilePrompt(profile.id)
  const resetPrompt = useResetProfilePrompt(profile.id)

  /** "…the default for the engineer role", in every confirmation below. */
  const role = roleLabel(profile.role).toLowerCase()

  return (
    <section className="flex flex-col gap-5">
      <h4 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Prompts</h4>

      <PromptEditor
        label="System prompt"
        hint="Prepended to whatever Ariadne tells the agent about its task."
        content={profile.system_prompt}
        restoreDescription={`The system prompt goes back to Ariadne's default for the ${role} role. Anything typed into the box and not saved is replaced.`}
        // A partial update: every field left out of the body stays as it is.
        onSave={(content) =>
          updateProfile.mutateAsync({ id: profile.id, body: { system_prompt: content } })
        }
        onRestore={() => resetSystemPrompt.mutateAsync().then((updated) => updated.system_prompt)}
      />

      {prompts.isPending ? (
        <LoadingPrompts />
      ) : prompts.isError ? (
        <ErrorState
          title="Could not load the prompts"
          error={prompts.error}
          onRetry={() => void prompts.refetch()}
        />
      ) : (
        prompts.data.map((prompt) => (
          <PromptEditor
            key={prompt.kind}
            label={PROMPT_KIND_LABELS[prompt.kind]}
            hint={PROMPT_KIND_HINTS[prompt.kind]}
            content={prompt.content}
            restoreDescription={`This briefing goes back to Ariadne's default for the ${role} role. Anything typed into the box and not saved is replaced.`}
            onSave={(content) => updatePrompt.mutateAsync({ kind: prompt.kind, content })}
            onRestore={() => resetPrompt.mutateAsync(prompt.kind).then((reset) => reset.content)}
          />
        ))
      )}
    </section>
  )
}

/** Standing in for prompts whose count is not known until they arrive. */
function LoadingPrompts() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-4 w-40" />
      <Skeleton className="h-24 w-full" />
    </div>
  )
}
