/**
 * Where a request can be reviewed or babysat: the hosts that have its
 * repository registered.
 *
 * A request is read off the forge by the host that lists it, but the agent
 * that works on it runs on a host with a checkout of its repository. Each host
 * names that checkout by an id of its own, so the choice is a host and the id
 * it goes with. The row's own host comes first and is the default.
 */

import { type Host, type PullRequestDto, useHosts } from "@/api"
import { Field, FieldLabel } from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { forgeKey, useHostRepositories } from "@/features/forge/forge-hosts"

/** One host a request can be worked on, and the id of its repository there. */
export interface ReviewTarget {
  host: Host
  repository_id: string
}

export function useReviewTargets(pull: PullRequestDto, host: Host, enabled = true): ReviewTarget[] {
  const { hosts } = useHosts()
  const reachable = enabled
    ? hosts.filter((each) => each.url !== null).map((each) => each.name)
    : []
  const repositories = useHostRepositories(reachable)
  const own = repositories.rows.find((row) => row.host === host && row.id === pull.repository_id)
  const key = own ? forgeKey(own) : null
  const matches = repositories.rows
    .filter((row) =>
      row.host === host ? row.id === pull.repository_id : key !== null && forgeKey(row) === key,
    )
    .map((row) => ({ host: row.host, repository_id: row.id }))
  // The row's own host first, whatever order the lists answered in.
  const ordered = [
    ...matches.filter((target) => target.host === host),
    ...matches.filter((target) => target.host !== host),
  ]
  // Until the lists answer, the request's own checkout is the one there is.
  return ordered.length > 0 ? ordered : [{ host, repository_id: pull.repository_id }]
}

/** The host select of the Start review and Start babysit dialogs; one host is no choice. */
export function ReviewHostSelect({
  id,
  targets,
  value,
  onChange,
}: {
  id: string
  targets: ReviewTarget[]
  value: Host
  onChange: (host: Host) => void
}) {
  if (targets.length < 2) return null
  return (
    <Field>
      <FieldLabel htmlFor={id}>Host</FieldLabel>
      <Select value={value} onValueChange={(next) => next && onChange(next)}>
        <SelectTrigger id={id} aria-label="Host" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {targets.map((target) => (
            <SelectItem key={target.host} value={target.host}>
              {target.host}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}
