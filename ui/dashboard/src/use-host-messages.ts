/**
 * The host's own half of the protocol `routes/dashboards.tsx` documents:
 * `{type: "theme", theme}` and `{type: "reload"}`, `postMessage`d from the
 * parent window. Same-origin by the content security policy's own
 * `frame-src 'self'`, so the origin check is a sanity guard rather than the
 * security boundary.
 */

import { useEffect } from "react"

interface HostMessageHandlers {
  onTheme: (theme: string) => void
  onReload: () => void
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null
}

export function useHostMessages({ onTheme, onReload }: HostMessageHandlers): void {
  useEffect(() => {
    function listener(event: MessageEvent): void {
      if (event.origin !== window.location.origin) return
      const data = event.data
      if (!isRecord(data)) return
      if (data.type === "theme" && typeof data.theme === "string") onTheme(data.theme)
      else if (data.type === "reload") onReload()
    }
    window.addEventListener("message", listener)
    return () => window.removeEventListener("message", listener)
  }, [onTheme, onReload])
}
