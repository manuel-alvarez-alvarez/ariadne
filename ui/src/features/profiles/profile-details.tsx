/**
 * Everything a profile holds, laid out for reading — the UI's `profile inspect`.
 *
 * The system prompt is the reason this is a panel rather than more columns: it
 * is the long, whitespace-significant field, so it is rendered verbatim in the
 * monospace face it was written in.
 */

import type { ReactNode } from "react"

import type { ProfileDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatAbsolute } from "@/lib/time"

import { agentKindLabel, modelLabel, roleLabel } from "./profile-labels"

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
          <CopyableId value={profile.id} label="profile id" className="text-xs" />
        </Detail>
        <Detail label="Created">{formatAbsolute(profile.created_at)}</Detail>
        <Detail label="Updated">{formatAbsolute(profile.updated_at)}</Detail>
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
        {/* The box that scrolls is a named region and takes focus: a prompt
            long enough to need a scrollbar is one only a pointer could read
            otherwise. */}
        <section
          aria-label="System prompt"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className="max-h-96 overflow-auto rounded-lg border bg-muted/40 p-3 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <pre className="font-mono text-xs leading-relaxed whitespace-pre-wrap">
            {profile.system_prompt}
          </pre>
        </section>
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
  // The value is truncated, so what the row says in full is only ever readable
  // in the hint — a `Tooltip` on a focusable `<dd>` rather than a `title=`,
  // which a keyboard never opens.
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
    </div>
  )
}
