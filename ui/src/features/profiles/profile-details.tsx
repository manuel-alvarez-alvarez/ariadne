/**
 * Everything a profile holds, laid out for reading — the UI's `profile inspect`.
 *
 * The system prompt is the reason this is a panel rather than more columns: it
 * is the long, whitespace-significant field, so it is rendered verbatim in the
 * monospace face it was written in.
 */

import type { ReactNode } from "react"

import type { ProfileDto } from "@/api"
import { Badge } from "@/components/ui/badge"

import { agentKindLabel, formatTimestamp, modelLabel, roleLabel } from "./profile-labels"

export function ProfileDetails({ profile }: { profile: ProfileDto }) {
  return (
    <div className="flex flex-col gap-5 py-2">
      <dl className="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-3">
        <Detail label="Role">{roleLabel(profile.role)}</Detail>
        <Detail label="Agent" unset={!profile.agent_kind} hint="first installed CLI, at spawn time">
          {agentKindLabel(profile.agent_kind)}
        </Detail>
        <Detail label="Model" unset={!profile.model} hint="whatever the agent CLI uses">
          <span className="font-mono">{modelLabel(profile.model)}</span>
        </Detail>
        <Detail label="Id">
          <span className="font-mono text-xs">{profile.id}</span>
        </Detail>
        <Detail label="Created">{formatTimestamp(profile.created_at)}</Detail>
        <Detail label="Updated">{formatTimestamp(profile.updated_at)}</Detail>
      </dl>

      <section className="flex flex-col gap-2">
        <h4 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Extra flags
        </h4>
        {profile.extra_flags.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {profile.extra_flags.map((flag) => (
              <Badge key={flag} variant="outline" className="font-mono">
                {flag}
              </Badge>
            ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            None — the agent CLI is spawned with Ariadne's own arguments only.
          </p>
        )}
      </section>

      <section className="flex flex-col gap-2">
        <h4 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          System prompt
        </h4>
        <pre className="max-h-96 overflow-auto rounded-lg border bg-muted/40 p-3 font-mono text-xs leading-relaxed whitespace-pre-wrap">
          {profile.system_prompt}
        </pre>
      </section>
    </div>
  )
}

function Detail({
  label,
  children,
  unset = false,
  hint,
}: {
  label: string
  children: ReactNode
  /** True when the daemon holds no value and `children` is the standing-in word. */
  unset?: boolean
  /** What the daemon does instead, shown next to that word. */
  hint?: string
}) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <dt className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{label}</dt>
      <dd className="truncate text-sm" title={unset ? hint : undefined}>
        {unset ? <span className="text-muted-foreground italic">{children}</span> : children}
        {unset && hint ? (
          <span className="ml-1.5 text-xs text-muted-foreground">({hint})</span>
        ) : null}
      </dd>
    </div>
  )
}
