/**
 * Open an external URL the way a plain `<a href>` would: a real anchor,
 * clicked once and dropped. The app's own click handler
 * (`src/hooks/use-open-external-links.ts`) intercepts that click and hands
 * the URL to the Tauri opener plugin; outside Tauri, the anchor's own
 * default navigation opens it in a new tab.
 *
 * For the few places that answer an attention item's `Forge` target from an
 * imperative callback rather than a render — a toast action, the command
 * palette's own pick. Everywhere a target can be rendered as a link,
 * render a plain `<a href>` instead (`ui/AGENTS.md`).
 */
export function openExternalUrl(url: string): void {
  const anchor = document.createElement("a")
  anchor.href = url
  anchor.target = "_blank"
  anchor.rel = "noopener noreferrer"
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
}
