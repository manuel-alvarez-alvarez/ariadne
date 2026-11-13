/**
 * The icon of one coding agent.
 *
 * Three sources, in order: the SVG bundled for the agents Ariadne knows by
 * id, the `icon` URL the ACP registry gives, and the first letter of the id.
 * An image that fails to load hands over to the next source, so an offline
 * machine or a dead URL ends at the letter rather than a broken-image box.
 *
 * The bundled files sit under `public/` and are drawn by path, not imported:
 * the webview's `img-src` allows no `data:` URL, which is what a bundler
 * turns a small imported SVG into.
 */

import { useState } from "react"

import { cn } from "@/lib/format"

/** Registry agent id → the SVG bundled for it. */
const BUNDLED: Record<string, string> = {
  "claude-acp": "/agent-icons/claude.svg",
  "codex-acp": "/agent-icons/codex.svg",
  opencode: "/agent-icons/opencode.svg",
  "opencode-v2": "/agent-icons/opencode.svg",
  gemini: "/agent-icons/gemini.svg",
}

export function AgentIcon({
  agentId,
  icon,
  className,
  size = 16,
  label,
}: {
  agentId: string
  /** The registry's icon URL, where the daemon has one. */
  icon?: string | null
  className?: string
  /** Side of the square, in pixels. */
  size?: number
  /** The accessible name. Without one the icon is decorative. */
  label?: string
}) {
  // The first source that has not failed: a bundled image that fails to load
  // hands over to the registry URL, and that to the letter.
  const [failed, setFailed] = useState<string[]>([])
  const src = [BUNDLED[agentId], icon].find((url) => url && !failed.includes(url)) || undefined
  const a11y = label ? { role: "img", "aria-label": label } : { "aria-hidden": true as const }

  if (src) {
    return (
      <img
        src={src}
        alt={label ?? ""}
        width={size}
        height={size}
        onError={() => setFailed((urls) => [...urls, src])}
        className={cn("shrink-0 object-contain", className)}
        {...(label ? {} : { "aria-hidden": true })}
      />
    )
  }
  return (
    <span
      {...a11y}
      style={{ width: size, height: size, fontSize: Math.max(8, Math.round(size * 0.6)) }}
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-sm bg-muted font-medium text-muted-foreground uppercase leading-none",
        className,
      )}
    >
      {agentId.charAt(0)}
    </span>
  )
}
