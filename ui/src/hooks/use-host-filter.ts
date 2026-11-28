/**
 * What the header's host picker selected, as the hosts a screen reads.
 *
 * The pick is `hostFilter` in the settings store: a host's name, or `""` for
 * All. All means every host the app can reach — an unreachable host shows its
 * state in the picker rather than as an error on every screen. A picked host
 * is read even while it is down, so its screens say why it is empty.
 */

import { useMemo } from "react"

import { type Host, type HostView, LOCAL_HOST, useHosts } from "@/api"
import { useSettingsStore } from "@/stores/settings"

/** The hosts the picker selected, and which of them the app reads. */
interface HostSelection {
  isPending: boolean
  /** The hosts a screen reads: the selection's hosts that have a URL, or a picked host. */
  read: Host[]
  /** Every listed host the selection means; a saved pick that is not listed means All. */
  listed: HostView[]
  /** The listed hosts a screen does not read, each with `unreachable`, its reason. */
  absent: HostView[]
}

/** What the picker selected, as the hosts a screen reads and the ones it cannot. */
export function useHostSelection(): HostSelection {
  const pick = useSettingsStore((state) => state.hostFilter)
  const { hosts, isPending } = useHosts()
  const picked = pick !== "" ? hosts.find((host) => host.name === pick) : undefined
  // A pick the settings daemon has not listed yet waits for it; one it no
  // longer lists falls back to All.
  const listed = isPending ? [] : picked ? [picked] : hosts
  const names =
    !isPending && picked
      ? [picked.name]
      : listed.filter((host) => host.url !== null).map((host) => host.name)
  // One array per selection, so a list over it keeps its identity.
  const key = names.join("\n")
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is `names`, spelled stably
  const read = useMemo(() => names, [key])
  return {
    isPending,
    read,
    listed,
    absent: listed.filter((host) => !read.includes(host.name)),
  }
}

/** The names of the hosts the picker selected. */
export function useHostFilter(): Host[] {
  return useHostSelection().read
}

/**
 * Every host the app can reach, whatever the pick: what the attention list and
 * the command palette read, so one pick never hides the items of another host.
 */
export function useReachableHosts(): Host[] {
  const { hosts } = useHosts()
  const names = hosts.filter((host) => host.url !== null).map((host) => host.name)
  // One array per set of hosts, so a list over it keeps its identity.
  const key = names.join("\n")
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is `names`, spelled stably
  return useMemo(() => names, [key])
}

/**
 * The one host the picker names: `local` under All, or for a pick the
 * settings daemon no longer lists. Null while a named pick waits for the
 * hosts list, so nothing reads `local` in its place first.
 */
export function usePickedHost(): Host | null {
  const pick = useSettingsStore((state) => state.hostFilter)
  const { hosts, isPending } = useHosts()
  if (isPending) return null
  if (hosts.some((host) => host.name === pick)) return pick
  return LOCAL_HOST
}

/** Whether the settings daemon lists a host besides `local`: the host controls show only then. */
export function useHasNamedHosts(): boolean {
  const { hosts } = useHosts()
  return hosts.some((host) => host.name !== LOCAL_HOST)
}
