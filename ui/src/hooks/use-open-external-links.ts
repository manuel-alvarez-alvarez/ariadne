/**
 * The Tauri webview ignores `target="_blank"` and has no shell opener wired
 * in by default, so a click on an external link otherwise does nothing (see
 * `ui/AGENTS.md`). One document-level listener, bound once by the shell,
 * opens it through `tauri-plugin-opener` instead.
 *
 * Outside Tauri — Vite's dev server, and every test — `isTauri()` is false
 * and the click is left alone, so a plain browser keeps its normal link
 * behavior (including `target="_blank"`). The terminal calls the returned
 * opener for OSC 8 links, which has no anchor text to compare with its target.
 */

import { isTauri } from "@tauri-apps/api/core"
import { openUrl } from "@tauri-apps/plugin-opener"
import { createElement, Fragment, type ReactNode, useCallback, useEffect, useState } from "react"

import { ConfirmDialog } from "@/components/confirm-dialog"

const EXTERNAL_PROTOCOLS = new Set(["http:", "https:", "mailto:"])

interface ExternalLink {
  href: string
}

interface OpenExternalLinks {
  /** Opens a URL only after the source text has made its destination clear. */
  openExternalLink: (href: string, visibleText?: string) => void
  /** The confirmation dialog, rendered by the caller beside its UI. */
  confirmation: ReactNode
}

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

export function useOpenExternalLinks({
  listen = true,
}: {
  listen?: boolean
} = {}): OpenExternalLinks {
  const [pending, setPending] = useState<ExternalLink | null>(null)

  const openExternalLink = useCallback((href: string, visibleText?: string) => {
    if (!isTauri() || !isAllowedExternalUrl(href)) return
    if (visibleText === href) {
      void openUrl(href)
      return
    }
    setPending({ href })
  }, [])

  useEffect(() => {
    if (!listen || !isTauri()) return

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
      if (event.button === 0) openExternalLink(anchor.href, anchor.textContent ?? undefined)
    }

    document.addEventListener("click", onClick)
    return () => document.removeEventListener("click", onClick)
  }, [listen, openExternalLink])

  return {
    openExternalLink,
    confirmation: pending
      ? createElement(ConfirmDialog, {
          open: true,
          onClose: () => setPending(null),
          title: "Open external link?",
          description: createElement(
            Fragment,
            null,
            "Ariadne will open this link in your default browser.",
            createElement("br"),
            createElement("code", { className: "break-all" }, pending.href),
          ),
          confirmLabel: "Open link",
          onConfirm: () => {
            void openUrl(pending.href)
            setPending(null)
          },
        })
      : null,
  }
}
