/**
 * The colour a host name is drawn in, live through `host_updated`.
 *
 * It reads the override from the hosts query, so a patched row repaints every
 * place that draws the host at once. The rule itself is `hostColor`.
 */

import { useHosts } from "@/api"
import { hostColor } from "@/lib/host-color"
import type { HostHue } from "@/lib/host-colors"

export function useHostColor(name: string): HostHue {
  const { hosts } = useHosts()
  return hostColor(name, hosts.find((host) => host.name === name)?.color)
}
