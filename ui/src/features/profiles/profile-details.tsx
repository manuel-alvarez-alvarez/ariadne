/**
 * Everything a profile holds — the UI's `profile inspect`, plus the writes the
 * table has no room for.
 *
 * The prompts are the reason this is a panel rather than more columns: they are
 * the long, whitespace-significant fields, and the table has nowhere to put
 * them. They are shown here and edited nowhere but the profile dialog (see
 * {@link ProfilePrompts}).
 */

import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"

import type { ProfileDto } from "@/api"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"

import { agentKindLabel, modelLabel, roleLabel } from "./profile-labels"
import { ProfilePrompts } from "./profile-prompts"
import { modelsQueryOptions } from "./queries"

export function ProfileDetails({ profile }: { profile: ProfileDto }) {
  // What the pinned model can do, when the catalog knows it. A model the
  // catalog does not list — free text, or the catalog failing to load — shows
  // exactly as before, so nothing here waits on the request.
  const models = useQuery(modelsQueryOptions())
  const catalogModel = profile.model
    ? models.data?.find(
        (model) =>
          model.id === profile.model &&
          (!profile.agent_kind || model.agent_kind === profile.agent_kind),
      )
    : undefined

  return (
    <div className="flex flex-col gap-5 py-2">
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3">
        <Detail label="Role">{roleLabel(profile.role)}</Detail>
        <Detail label="Agent" unset={!profile.agent_kind} hint="first installed CLI, at spawn time">
          {agentKindLabel(profile.agent_kind)}
        </Detail>
        <Detail
          label="Model"
          unset={!profile.model}
          hint="whatever the agent CLI uses"
          caption={catalogModel?.description}
        >
          <span className="font-mono">{modelLabel(profile.model)}</span>
        </Detail>
        <Detail label="Created">
          <When at={profile.created_at} label="created" />
        </Detail>
        <Detail label="Updated">
          <When at={profile.updated_at} label="updated" />
        </Detail>
      </dl>

      <ProfilePrompts profile={profile} />
    </div>
  )
}

function Detail({
  label,
  children,
  unset = false,
  hint,
  caption,
}: {
  label: string
  children: ReactNode
  /** True when the daemon holds no value and `children` is the standing-in word. */
  unset?: boolean
  /** What the daemon does instead, shown next to that word. */
  hint?: string
  /** A blurb about the value, on its own wrapping line under it. */
  caption?: string | null
}) {
  // The value is truncated, so what the row says in full is only ever readable
  // in the hint — a `Tooltip` on a focusable `<dd>` rather than a `title=`,
  // which a keyboard never opens. A `caption` is the opposite: prose rather
  // than an identifier, so it gets its own line and wraps there in full.
  const value = (
    <>
      {unset ? <span className="text-muted-foreground italic">{children}</span> : children}
      {unset && hint ? (
        <span className="ml-1.5 text-xs text-muted-foreground">({hint})</span>
      ) : null}
    </>
  )

  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{label}</dt>
      {unset && hint ? (
        <Tooltip>
          <TooltipTrigger render={<dd className="truncate text-sm" />}>{value}</TooltipTrigger>
          <TooltipContent>{hint}</TooltipContent>
        </Tooltip>
      ) : (
        <dd className="truncate text-sm">{value}</dd>
      )}
      {caption ? <dd className="text-xs leading-snug text-muted-foreground">{caption}</dd> : null}
    </div>
  )
}
