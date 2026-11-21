/**
 * The colour of a host: the one rule every host name in the app is drawn by.
 *
 * A host's own `HostDto.color` wins. Without one, `local` is grey and a named
 * host takes one of the eight colours by a stable hash of its name, so a name
 * always has the same colour. The tokens are `--host-<name>` in `index.css`.
 */

import { HOST_COLORS, type HostColor, type HostHue } from "./host-colors"

/** FNV-1a over the name's UTF-16 units: stable across runs and builds. */
function hashName(name: string): number {
  let hash = 0x811c9dc5
  for (let i = 0; i < name.length; i++) {
    hash ^= name.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193)
  }
  return hash >>> 0
}

/** The colour of a host, given its name and its `color` override. */
export function hostColor(name: string, override?: HostColor | null): HostHue {
  if (override) return override
  if (name === "local") return "grey"
  return HOST_COLORS[hashName(name) % HOST_COLORS.length]?.value ?? "grey"
}
