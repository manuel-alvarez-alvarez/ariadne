/**
 * What the Forge tabs read across hosts.
 *
 * A repository's id is its daemon's own, so the same forge repository has one
 * id per host that registered it. The forge name — kind, owner and name — is
 * what the hosts share: a filter on one repository, and the hosts a request can
 * be reviewed on, both go through it.
 */

import { type Host, type HostRow, type RepositoryDto, useHostLists } from "@/api"
import { repositoriesQueryOptions } from "@/features/repositories/queries"

/** What names one repository on every host, or null for one with no forge. */
export function forgeKey(repository: RepositoryDto): string | null {
  const { forge } = repository
  return forge ? `${forge.kind}:${forge.owner}/${forge.name}` : null
}

/** The registered repositories of every host in `hosts`, each tagged with its host. */
export function useHostRepositories(hosts: readonly Host[]) {
  return useHostLists(hosts, repositoriesQueryOptions)
}

/** One repository id per host: the checkout of the forge repository `repository` names. */
export function repositoryOn(
  rows: readonly HostRow<RepositoryDto>[],
  repository: string,
  host: Host,
): string | undefined {
  const known = rows.find((row) => row.id === repository)
  // Before the lists have answered, the id is taken as the host's own.
  if (!known) return repository
  const key = forgeKey(known)
  if (known.host === host) return known.id
  if (key === null) return undefined
  return rows.find((row) => row.host === host && forgeKey(row) === key)?.id
}

/** The enabled repositories, one per forge repository: the filter's choices. */
export function distinctEnabled(rows: readonly HostRow<RepositoryDto>[]): HostRow<RepositoryDto>[] {
  const seen = new Set<string>()
  return rows.filter((row) => {
    if (!row.forge?.enabled) return false
    const key = forgeKey(row) ?? row.id
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}
