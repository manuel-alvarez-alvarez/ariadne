/**
 * The one rule for when a row or a panel names its host.
 *
 * A host label tells hosts apart, so it shows only where the settings daemon
 * lists a named host beside `local`. The rule reads the listed hosts, not the
 * reachable ones: a host that goes down stays listed, so the labels of the
 * rows that stay do not come and go with it.
 */

import { LOCAL_HOST, useHosts } from "@/api"
import { useSettingsStore } from "@/stores/settings"

/** True when the settings daemon lists a host other than `local`. */
function useNamedHostListed(): boolean {
  const { hosts } = useHosts()
  return hosts.some((host) => host.name !== LOCAL_HOST)
}

/**
 * Whether each row names its host: under All, with a named host listed. A
 * saved pick the settings daemon no longer lists counts as All, since the
 * rows then come from every host (`useHostFilter`).
 */
export function useRowsShowHost(): boolean {
  const pick = useSettingsStore((state) => state.hostFilter)
  const { hosts } = useHosts()
  const named = useNamedHostListed()
  const pickedOne = pick !== "" && hosts.some((host) => host.name === pick)
  return named && !pickedOne
}

/** Whether a goal, task or session panel names its host: with a named host listed. */
export function usePanelShowsHost(): boolean {
  return useNamedHostListed()
}
