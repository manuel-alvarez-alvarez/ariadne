/**
 * The Tauri webview ignores `target="_blank"` and has no shell opener wired
 * in by default, so a click on an external link otherwise does nothing (see
 * `ui/AGENTS.md`). One document-level listener, bound once by the shell,
 * opens it through `tauri-plugin-opener` instead.
 *
 * Outside Tauri — Vite's dev server, and every test — `isTauri()` is false
 * and the click is left alone, so a plain browser keeps its normal link
 * behavior (including `target="_blank"`).
 */

import { isTauri } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { useEffect } from "react"

/** An anchor is external when it does not resolve onto the app's own origin. */
function isExternalAnchor(anchor: HTMLAnchorElement): boolean {
  if (anchor.protocol === "mailto:") return true
  return (
    (anchor.protocol === "http:" || anchor.protocol === "https:") &&
    anchor.origin !== window.location.origin
  )
}

export function useOpenExternalLinks(): void {
  useEffect(() => {
    if (!isTauri()) return

    function onClick(event: MouseEvent) {
      if (event.defaultPrevented || event.button !== 0) return
      const anchor = (event.target as Element | null)?.closest("a")
      if (!anchor || !isExternalAnchor(anchor)) return

      event.preventDefault()
      void openUrl(anchor.href)
    }

    document.addEventListener("click", onClick)
    return () => document.removeEventListener("click", onClick)
  }, [])
}
