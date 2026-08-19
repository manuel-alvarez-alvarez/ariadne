/**
 * A profile in one line: `name · agent kind · model`.
 *
 * The three surfaces that mention a profile — a session's summary, the
 * sessions table, the task panel's Engineer and Reviewers — each used to say
 * something different about it: the name alone, the name with the agent kind
 * hidden in a `title=`, the name and nothing else. Which agent CLI and which
 * model an agent is running is the question those mentions are actually there
 * to answer, so they answer it here, once.
 *
 * The name links to the profile ({@link ProfileName}); the two facts after it
 * are quiet secondary text, and the whole line truncates, since it sits in
 * table cells and 48rem panels.
 *
 * A profile is editable, so what a *session* runs is not necessarily what its
 * profile says today: a session records the agent kind and the model it was
 * launched with, and those win where there are any (see {@link launched}).
 */

import { useQuery } from "@tanstack/react-query"

import type { AgentKind } from "@/api"
import { cn } from "@/lib/utils"

import { agentKindLabel, modelLabel } from "./profile-labels"
import { ProfileName } from "./profile-name"
import { profilesQueryOptions } from "./queries"

export function ProfileSummary({
  profileId,
  launched,
  className,
}: {
  profileId: string
  /**
   * What was actually launched, for a profile shown as a session's: the
   * session's own snapshot, taken when it started. A null model there means
   * the agent CLI's default was used — not that the profile's model applies.
   */
  launched?: { agent_kind: AgentKind; model?: string | null }
  className?: string
}) {
  const profiles = useQuery(profilesQueryOptions())
  const profile = profiles.data?.find((item) => item.id === profileId)
  // Nothing to fall back on while the profiles are still loading: the facts
  // wait for the answer rather than claiming `auto · default` and flipping.
  const facts = launched ?? profile

  return (
    <span className={cn("flex min-w-0 items-baseline gap-1", className)}>
      <ProfileName profileId={profileId} className="min-w-0" />
      {facts ? (
        <span className="truncate text-muted-foreground">
          · {agentKindLabel(facts.agent_kind)} · {modelLabel(facts.model)}
        </span>
      ) : null}
    </span>
  )
}
