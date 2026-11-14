/**
 * The Tauri webview ignores `target="_blank"` and has no shell opener wired
 * in by default, so a click on an external link otherwise does nothing (see
 * `ui/AGENTS.md`). One document-level listener, bound once by the shell,
 * opens it through `tauri-plugin-opener` instead, with no confirmation.
 *
 * Outside Tauri — Vite's dev server, and every test — `isTauri()` is false
 * and the click is left alone, so a plain browser keeps its normal link
 * behavior (including `target="_blank"`).
 */

import { isTauri } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { useEffect } from "react"

const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "mailto:"])

/** An anchor is external when it does not resolve onto the app's own origin. */
function isExternalAnchor(anchor: HTMLAnchorElement): boolean {
  return anchor.origin !== window.location.origin
}

/** Whether the opener is allowed to hand this absolute URL to the operating system. */
function isAllowedExternalUrl(href: string): boolean {
  try {
    return EXTERNAL_PROTOCOLS.has(new URL(href, window.location.href).protocol)
  } catch {
    return false
  }
}

export function useOpenExternalLinks(): void {
  useEffect(() => {
    if (!isTauri()) return

    function onClick(event: MouseEvent) {
      if (event.defaultPrevented) return
      const anchor = (event.target as Element | null)?.closest("a")
      if (!anchor) return
      if (!isExternalAnchor(anchor)) return
      if (!isAllowedExternalUrl(anchor.href)) {
        event.preventDefault()
        return
      }

      event.preventDefault()
      if (event.button === 0) void openUrl(anchor.href)
    }

    document.addEventListener("click", onClick)
    return () => document.removeEventListener("click", onClick)
  }, [])
}
