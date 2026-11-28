/**
 * Where a request can be reviewed or babysat: the hosts that have its
 * repository registered.
 *
 * A request is read off the forge by the host that lists it, but the agent
 * that works on it runs on a host with a checkout of its repository. Each host
 * names that checkout by an id of its own, so the choice is a host and the id
 * it goes with. The row's own host comes first and is the default.
 */

import { useQueries } from "@tanstack/react-query"

import { type Host, type HostRow, type PullRequestDto, useHosts } from "@/api"
import { HostDot } from "@/components/host-dot"
import { Field, FieldLabel } from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { forgeKey, useHostRepositories } from "@/features/forge/forge-hosts"

import { pullRequestByKeyQueryOptions, pullRequestKey } from "./queries"

/** One host a request can be worked on, and the id of its repository there. */
interface ReviewTarget {
  host: Host
  repository_id: string
}

export function useReviewTargets(pull: PullRequestDto, host: Host, enabled = true): ReviewTarget[] {
  return useTargets(pull, host, enabled).targets
}

function useTargets(pull: PullRequestDto, host: Host, enabled: boolean) {
  const { hosts, isPending } = useHosts()
  const reachable = enabled
    ? hosts.filter((each) => each.url !== null).map((each) => each.name)
    : []
  const repositories = useHostRepositories(reachable)
  // With no other host to read, there is nothing to wait for.
  const alone = reachable.every((each) => each === host)
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
  return {
    // Until the lists answer, the request's own checkout is the one there is.
    targets:
      isPending || (!alone && !repositories.isSettled)
        ? []
        : ordered.length > 0
          ? ordered
          : [{ host, repository_id: pull.repository_id }],
    // The request's own host is read already: its list only finds the others.
    errors: repositories.errors.filter((each) => each.host !== host),
    /** The hosts are listed and every reachable host's repositories answered or failed. */
    settled: !isPending && (alone || repositories.isSettled),
    refetch: repositories.refetch,
  }
}

/**
 * The request as each host that has its repository reads it: the panel's own
 * copy first, then one read per other host, by that host's repository id.
 * A host whose repositories or request could not be read is an error of its
 * own, and the copies of the other hosts stay. `settled` says when no host is
 * left to answer.
 */
export function useRequestCopies(pull: PullRequestDto, host: Host) {
  const { targets, errors, settled, refetch } = useTargets(pull, host, true)
  const others = targets.filter((target) => target.host !== host)
  const reads = useQueries({
    queries: others.map((target) =>
      pullRequestByKeyQueryOptions(
        target.host,
        pullRequestKey({ repository_id: target.repository_id, number: pull.number }),
      ),
    ),
  })
  const copies: HostRow<PullRequestDto>[] = [
    { ...pull, host },
    ...reads.flatMap((read, index) =>
      read.data ? [{ ...read.data, host: others[index]?.host as Host }] : [],
    ),
  ]
  const failed = [
    ...errors.map(({ host, error }) => ({ host, error })),
    ...reads.flatMap((read, index) =>
      read.error ? [{ host: others[index]?.host as Host, error: read.error }] : [],
    ),
  ]
  return {
    copies,
    /**
     * Every host that has the repository was read, or failed: until then a
     * host not read yet may already review or keep the request.
     */
    settled: settled && reads.every((read) => !read.isPending),
    // One error per host, the first it met.
    errors: failed.filter((each, index) => failed.findIndex((e) => e.host === each.host) === index),
    refetch: () => {
      refetch()
      for (const read of reads) if (read.error) void read.refetch()
    },
  }
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
      <Select
        value={value}
        items={targets.map((target) => ({
          value: target.host,
          label: (
            <span className="flex items-center gap-2">
              <HostDot host={target.host} />
              {target.host}
            </span>
          ),
        }))}
        onValueChange={(next) => next && onChange(next)}
      >
        <SelectTrigger id={id} aria-label="Host" className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false}>
          {targets.map((target) => (
            <SelectItem key={target.host} value={target.host}>
              <span className="flex items-center gap-2">
                <HostDot host={target.host} />
                {target.host}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}
