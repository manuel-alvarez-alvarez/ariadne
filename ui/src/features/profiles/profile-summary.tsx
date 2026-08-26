/**
 * A profile in one line: `name · agent kind · model`.
 *
 * The surfaces that mention a profile — a session's summary, the sessions
 * table, the task panel's Engineer and Reviewers, the goal panel's Planner —
 * each used to say something different about it: the name alone, the name with
 * the agent kind hidden in a `title=`, the name and nothing else. Which agent
 * CLI and which model an agent is running is the question those mentions are
 * actually there to answer, so they answer it here, once.
 *
 * The name links to the profile ({@link ProfileName}); the two facts after it
 * are quiet secondary text, and the whole line truncates, since it sits in
 * table cells and 48rem panels.
 *
 * A profile is editable and what runs is not: a session records what it was
 * launched with, and a task, a reviewer slot and a goal each record what they
 * were created with. Those snapshots win over the profile wherever there is one
 * (see {@link pinned}) — which is the whole point, since the moment the two
 * disagree is the moment somebody edited the profile.
 */

import { useQuery } from "@tanstack/react-query"

import type { AgentKind } from "@/api"
import { cn } from "@/lib/format"

import { agentKindLabel, modelLabel } from "./profile-labels"
import { ProfileName } from "./profile-name"
import { profilesQueryOptions } from "./queries"

/**
 * Whether what this mention runs on is no longer what its profile says.
 *
 * True of a pin chosen for the task, the slot or the goal itself, and of one
 * the profile has since been edited away from — the two are the same fact from
 * the reader's side: what runs is the pin, and the profile is not it. Both
 * halves count, since choosing a model pins the agent CLI that runs it.
 */
export function isPinOverride(
  profile: { agent_kind?: AgentKind | null; model?: string | null } | undefined,
  pinned: { agent_kind?: AgentKind | null; model?: string | null } | undefined | null,
): boolean {
  if (!profile || !pinned) return false
  return (
    (pinned.agent_kind ?? null) !== (profile.agent_kind ?? null) ||
    (pinned.model ?? null) !== (profile.model ?? null)
  )
}

export function ProfileSummary({
  profileId,
  pinned,
  className,
}: {
  profileId: string
  /**
   * What this mention's agent actually runs on: a session's launch snapshot, a
   * task's or a reviewer slot's pin, a goal's. A null agent kind is `auto`
   * resolved at spawn time and a null model is the agent CLI's default —
   * neither falls back to what the profile says.
   */
  pinned?: { agent_kind?: AgentKind | null; model?: string | null }
  className?: string
}) {
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === profileId)
  // Nothing to fall back on while the profiles are still loading: the facts
  // wait for the answer rather than claiming `auto · default` and flipping.
  const facts = pinned ?? profile

  return (
    <span className={cn("flex min-w-0 items-baseline gap-1", className)}>
      <ProfileName profileId={profileId} className="min-w-0" />
      {facts ? (
        <span className="truncate text-muted-foreground">
          · {agentKindLabel(facts.agent_kind)} · {modelLabel(facts.model)}
          {/* Where the two disagree, the pair just read is the pin and not
              the profile's own — the model chosen here, or a profile edited
              since. One word, because the line sits in table cells and panel
              columns and already carries three facts. */}
          {isPinOverride(profile, pinned) ? " (overrides)" : null}
        </span>
      ) : null}
    </span>
  )
}
