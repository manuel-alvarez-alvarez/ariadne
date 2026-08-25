/**
 * Every prompt a profile is spawned with, shown as it takes effect.
 *
 * Which prompts those are is not decided here: `GET /v1/profiles/{id}/prompts`
 * answers exactly the kinds the profile's role owns, in briefing order, so a
 * planner shows one briefing and an engineer three without this file knowing
 * which. Only the system prompt is added on top — it lives on the profile
 * itself rather than in that list.
 *
 * A prompt is the profile's own text or the default of its kind, and each one
 * says which: a badge, from the daemon's flag rather than from comparing text
 * here. Putting one back on its default is the dialog's, like every other edit.
 *
 * Read-only on purpose: prompts are edited in one place, the profile dialog
 * (see {@link import("./profile-form-dialog").ProfileFormDialog}), which the
 * row's Edit button opens. This panel only displays them.
 *
 * The list is fetched when the details panel opens, which is the only time it
 * is on screen.
 */

import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"

import type { ProfileDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Badge } from "@/components/ui/badge"
import { FieldDescription, FieldTitle } from "@/components/ui/field"
import { Skeleton } from "@/components/ui/skeleton"

import { PROMPT_KIND_HINTS, PROMPT_KIND_LABELS } from "./profile-labels"
import { profilePromptsQueryOptions } from "./queries"

export function ProfilePrompts({ profile }: { profile: ProfileDto }) {
  const prompts = useQuery(profilePromptsQueryOptions(profile.id))

  return (
    <section className="flex flex-col gap-5">
      <h4 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Prompts</h4>

      <ReadOnlyPrompt
        label="System prompt"
        hint="Prepended to whatever Ariadne tells the agent about its task."
        content={profile.system_prompt}
        isDefault={profile.system_prompt_is_default}
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
          <ReadOnlyPrompt
            key={prompt.kind}
            label={PROMPT_KIND_LABELS[prompt.kind]}
            hint={PROMPT_KIND_HINTS[prompt.kind]}
            content={prompt.content}
            isDefault={prompt.is_default}
          />
        ))
      )}
    </section>
  )
}

/**
 * One prompt as the panel shows it: its name, whether the text is the profile's
 * own or the default of its kind, its text, and when it is sent.
 *
 * A briefing is long, so the block is capped and scrolls on its own — four of
 * them in one expanded row would otherwise make the row endless. The text is
 * whitespace-significant and read back as a template, hence monospace and
 * `pre-wrap`; the block is a labelled region, since there is no control here
 * for the name to label.
 */
function ReadOnlyPrompt({
  label,
  hint,
  content,
  isDefault,
}: {
  label: string
  hint: ReactNode
  content: string
  /** Whether the profile runs on the default of this prompt. */
  isDefault: boolean
}) {
  return (
    <div className="flex w-full flex-col gap-2">
      <div className="flex items-center gap-2">
        <FieldTitle>{label}</FieldTitle>
        <Badge variant="outline">{isDefault ? "default" : "edited"}</Badge>
      </div>
      <section
        aria-label={label}
        className="max-h-96 overflow-auto rounded-md border bg-muted/20 px-3 py-2"
      >
        <pre className="font-mono text-xs leading-relaxed break-words whitespace-pre-wrap">
          {content}
        </pre>
      </section>
      <FieldDescription className="text-xs">{hint}</FieldDescription>
    </div>
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
