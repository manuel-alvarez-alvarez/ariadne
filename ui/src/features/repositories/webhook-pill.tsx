/**
 * The repositories table's Webhook cell (026, 027): one pill that says how
 * the repository's requests reach Ariadne, and whether that works.
 *
 * `localtunnel` while the webhook tunnel is switched on: the forge pushes
 * each change through it, and the pill is green only while the hook is
 * live. `polling` while it is off: Ariadne fetches on a timer, and the pill
 * is green only while the last fetch worked. The tooltip says the rest — the
 * URL, the error, and how to turn the tunnel on.
 */

import type { ForgeDto, ForgeTunnelDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

/** How the requests of an enabled integration arrive, and whether that works. */
function delivery(forge: ForgeDto, tunnel: ForgeTunnelDto | undefined) {
  const { webhook } = forge
  // While the tunnel is still loading, the hook itself says which it is.
  const tunnelled = tunnel ? tunnel.enabled : webhook.state === "live"
  const working = tunnelled
    ? webhook.state === "live" && !webhook.fetch_error
    : !webhook.fetch_error
  return { mode: tunnelled ? "localtunnel" : "polling", working }
}

export function WebhookPill({
  forge,
  tunnel,
}: {
  forge: ForgeDto
  /** The tunnel's switch and state; undefined while it is still loading. */
  tunnel: ForgeTunnelDto | undefined
}) {
  // A disabled integration neither listens nor fetches.
  if (!forge.enabled) return <span className="text-muted-foreground">-</span>
  const { mode, working } = delivery(forge, tunnel)
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Badge
            role="img"
            aria-label={`${mode}: ${working ? "working" : "not working"}`}
            className={cn(
              "gap-1.5",
              working
                ? "bg-status-done-soft text-status-done-fg"
                : "bg-status-danger-soft text-status-danger-fg",
            )}
          />
        }
      >
        <span
          aria-hidden="true"
          className={cn("size-1.5 rounded-full", working ? "bg-status-done" : "bg-status-danger")}
        />
        {mode}
      </TooltipTrigger>
      <TooltipContent>
        <Explanation forge={forge} tunnel={tunnel} mode={mode} working={working} />
      </TooltipContent>
    </Tooltip>
  )
}

function Explanation({
  forge,
  tunnel,
  mode,
  working,
}: {
  forge: ForgeDto
  tunnel: ForgeTunnelDto | undefined
  mode: string
  working: boolean
}) {
  const { webhook } = forge
  const lines: { text: string; mono?: boolean }[] = []
  if (mode === "localtunnel" && working) {
    lines.push({ text: "The webhook is live: changes arrive the moment they happen." })
    if (webhook.url) lines.push({ text: webhook.url, mono: true })
  } else if (mode === "localtunnel") {
    lines.push({
      text: "The webhook is not working. Ariadne checks the forge every 5 minutes until it works.",
    })
    if (tunnel?.state === "down" && tunnel.error) {
      lines.push({ text: `Tunnel: ${tunnel.error}` })
    }
    if (webhook.error) lines.push({ text: `Webhook: ${webhook.error}` })
    if (webhook.fetch_error) lines.push({ text: `Last fetch: ${webhook.fetch_error}` })
  } else if (working) {
    lines.push({ text: "Ariadne checks the forge for changes every 5 minutes." })
    lines.push({
      text: "Turn on the webhook tunnel in Settings (the gear in the header) to get them the moment they happen.",
    })
  } else {
    lines.push({
      text: "Ariadne could not read the forge. It tries again every 5 minutes.",
    })
    if (webhook.fetch_error) lines.push({ text: webhook.fetch_error })
  }
  return (
    <span className="flex flex-col gap-0.5 break-words">
      {lines.map((line) => (
        <span key={line.text} className={line.mono ? "font-mono break-all" : undefined}>
          {line.text}
        </span>
      ))}
    </span>
  )
}
