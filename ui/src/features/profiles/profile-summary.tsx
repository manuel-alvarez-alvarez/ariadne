/**
 * A profile in one line: `name · model`.
 *
 * The surfaces that mention a profile — a session's summary, the sessions
 * table, the task panel's Engineer and Reviewers, the goal panel's Planner —
 * each used to say something different about it: the name alone, the name with
 * the agent kind hidden in a `title=`, the name and nothing else. What an agent
 * is running is the question those mentions are actually there to answer, so
 * they answer it here, once — and since a model names the agent CLI that runs
 * it (`claude_code:claude-opus-5`), that is one fact and not two.
 *
 * The name links to the profile ({@link ProfileName}); the model after it is
 * quiet secondary text, and the whole line truncates, since it sits in table
 * cells and 48rem panels.
 *
 * A profile is editable and what runs is not: a session records what it was
 * launched with, and a task, a reviewer slot and a goal each record what they
 * were created with. Those snapshots win over the profile wherever there is one
 * (see {@link model}) — which is the whole point, since the moment the two
 * disagree is the moment somebody edited the profile. A session is the one
 * mention whose snapshot is still two fields on the wire, so its caller
 * composes the id with {@link formatModelRef} before handing it here.
 */

import { useQuery } from "@tanstack/react-query"

import { cn } from "@/lib/format"

import { modelRefLabel } from "./model-ref"
import { ProfileName } from "./profile-name"
import { profilesQueryOptions } from "./queries"

/**
 * Whether what this mention runs on is no longer what its profile says.
 *
 * True of a pin chosen for the task, the slot or the goal itself, and of one
 * the profile has since been edited away from — the two are the same fact from
 * the reader's side: what runs is the pin, and the profile is not it. Nothing
 * to compare against — a mention that carries no snapshot at all — is never an
 * override.
 */
export function isPinOverride(
  profile: { model?: string | null } | undefined,
  pinned: string | null | undefined,
): boolean {
  if (!profile || pinned === undefined) return false
  return (pinned ?? null) !== (profile.model ?? null)
}

export function ProfileSummary({
  profileId,
  model,
  className,
}: {
  profileId: string
  /**
   * What this mention's agent actually runs on, as the one qualified id that
   * says it: a session's launch snapshot, a task's or a reviewer slot's pin, a
   * goal's. Null is `auto` — the first installed CLI, resolved at spawn time,
   * on its own default model — and does not fall back to what the profile
   * says; undefined is a mention with no snapshot of its own, which does.
   */
  model?: string | null
  className?: string
}) {
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === profileId)
  // Nothing to fall back on while the profiles are still loading: the fact
  // waits for the answer rather than claiming `auto` and flipping.
  const pinned = model === undefined ? profile?.model : model
  const known = model !== undefined || profile !== undefined

  return (
    <span className={cn("flex min-w-0 items-baseline gap-1", className)}>
      <ProfileName profileId={profileId} className="min-w-0" />
      {known ? (
        <span className="truncate text-muted-foreground">
          · {modelRefLabel(pinned)}
          {/* Where the two disagree, what was just read is the pin and not the
              profile's own — the model chosen here, or a profile edited since.
              One word, because the line sits in table cells and panel columns
              and already carries two facts. */}
          {isPinOverride(profile, model) ? " (overrides)" : null}
        </span>
      ) : null}
    </span>
  )
}
