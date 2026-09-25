/**
 * The three ways a repository answers its agents' ACP permission requests, in
 * the order the picker lists them, and the one line each means on screen.
 */

import type { PermissionMode } from "@/api"

export const PERMISSION_MODES: { value: PermissionMode; label: string; meaning: string }[] = [
  { value: "auto", label: "Auto", meaning: "approve every request without asking" },
  { value: "ask", label: "Ask", meaning: "wait for an answer in the session console" },
  {
    value: "learn",
    label: "Learn",
    meaning: "ask once per tool, then remember an approval",
  },
]

/** The label a mode is shown with. */
export function permissionModeLabel(mode: PermissionMode): string {
  return PERMISSION_MODES.find((one) => one.value === mode)?.label ?? mode
}
