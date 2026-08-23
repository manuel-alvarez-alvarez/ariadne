/**
 * The profiles a thread's compose box may address, named.
 *
 * Which ids those are belongs to each thread — the daemon's rule is in
 * `http/recipients.rs`, and mirroring it is what keeps the picker from offering
 * an addressee the daemon would refuse. What belongs here is turning them into
 * the picker's options: names, read off the profiles list every other id
 * mention on screen reads (see {@link ProfileName}), so one request serves them
 * all and whichever screen loads it first serves the rest.
 *
 * The order given is kept — it is the order the daemon lists participants in —
 * and an id repeated in it is offered once. A profile the list has no row for
 * is left out rather than offered as a ULID: it was deleted since the thread
 * named it, and there is no one behind it to read the message.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import type { Addressee } from "@/components/message-composer"
import { profilesQueryOptions } from "./queries"

export function useAddressees(profileIds: readonly string[]): Addressee[] {
  const profiles = useQuery(profilesQueryOptions())
  // The callers build their id list inline, so it is a new array every render;
  // the joined key is what actually changed, and memoizing on it keeps one
  // options array alive for as long as the thread's participants are the same.
  const key = profileIds.join(",")
  return useMemo(() => {
    const names = new Map((profiles.data ?? []).map((profile) => [profile.id, profile.name]))
    const addressees: Addressee[] = []
    for (const id of key ? key.split(",") : []) {
      const name = names.get(id)
      if (!name || addressees.some((addressee) => addressee.id === id)) continue
      addressees.push({ id, name })
    }
    return addressees
  }, [profiles.data, key])
}
