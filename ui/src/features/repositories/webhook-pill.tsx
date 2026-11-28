/**
 * The repositories table's Webhook cell (026, 027): one pill that says how
 * the repository's requests reach Ariadne, and whether that works.
 *
 * `smee` while the webhook tunnel is switched on: the forge pushes
 * each change through it, and the pill is green only while the hook is
 * live. `polling` while it is off: Ariadne fetches on a timer, and the pill
 * is green only while the last fetch worked. The tooltip says the rest — the
 * URL, the error, and how to turn the tunnel on.
 *
 * A hook the forge refused for rights reads `polling` in amber: the fetch
 * works, and each timer fetch retries the hook. The tooltip says how to grant
 * the rights, and how to stop the retries.
 */

import type { ForgeDto, ForgeTunnelDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

/** How the requests of an enabled integration arrive, and whether that works. */
function delivery(forge: ForgeDto, tunnel: ForgeTunnelDto | undefined) {
  const { webhook } = forge
  // A hook the forge refused for rights polls until a retry registers it.
  const refused = forge.webhook_enabled && webhook.rights_refused === true
  // While the tunnel is still loading, the hook itself says which it is.
  // A repository whose webhook is off polls, whatever the tunnel does.
  const tunnelled =
    forge.webhook_enabled && !refused && (tunnel ? tunnel.enabled : webhook.state === "live")
  const working = tunnelled
    ? webhook.state === "live" && !webhook.fetch_error
    : !webhook.fetch_error
  return { mode: tunnelled ? "smee" : "polling", working, refused }
}

/**
 * Whether the forge pushes every change of an enabled integration the moment
 * it happens: the tunnel is on and the hook is live. Anything else — polling,
 * or a tunnel or hook that is down — leaves a list up to five minutes behind,
 * so the Forge tabs offer a refresh only then.
 */
export function pushesLive(forge: ForgeDto, tunnel: ForgeTunnelDto | undefined) {
  const { mode, working } = delivery(forge, tunnel)
  return mode === "smee" && working
}

export function WebhookPill({
  forge,
  tunnel,
  failed = false,
}: {
  forge: ForgeDto
  /** The tunnel's switch and state; undefined while it is still loading. */
  tunnel: ForgeTunnelDto | undefined
  /** The tunnel could not be read: its mode is unknown, not loading. */
  failed?: boolean
}) {
  // A disabled integration neither listens nor fetches.
  if (!forge.enabled) return <span className="text-muted-foreground">-</span>
  // A repository whose webhook is off polls, and one the forge refused for
  // rights polls too: neither waits for a tunnel answer.
  const waiting = forge.webhook_enabled && forge.webhook.rights_refused !== true && !tunnel
  if (waiting && failed)
    return (
      <span className="text-muted-foreground" title="The tunnel could not be read">
        unknown
      </span>
    )
  if (waiting) return <Skeleton aria-label="Loading webhook mode" className="h-5 w-16" />
  const { mode, working, refused } = delivery(forge, tunnel)
  const calm = working && refused
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Badge
            role="img"
            aria-label={`${mode}: ${calm ? "no webhook rights" : working ? "working" : "not working"}`}
            className={cn(
              "gap-1.5",
              calm
                ? "bg-status-warn-soft text-status-warn-fg"
                : working
                  ? "bg-status-done-soft text-status-done-fg"
                  : "bg-status-danger-soft text-status-danger-fg",
            )}
          />
        }
      >
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 rounded-full",
            calm ? "bg-status-warn" : working ? "bg-status-done" : "bg-status-danger",
          )}
        />
        {mode}
      </TooltipTrigger>
      <TooltipContent>
        <Explanation
          forge={forge}
          tunnel={tunnel}
          mode={mode}
          working={working}
          refused={refused}
        />
      </TooltipContent>
    </Tooltip>
  )
}

/** The minutes between timer fetches of a repository: its own, else 5. */
function intervalMinutes(forge: ForgeDto): number {
  return forge.poll_interval_minutes ?? 5
}

function Explanation({
  forge,
  tunnel,
  mode,
  working,
  refused,
}: {
  forge: ForgeDto
  tunnel: ForgeTunnelDto | undefined
  mode: string
  working: boolean
  refused: boolean
}) {
  const { webhook } = forge
  const minutes = intervalMinutes(forge)
  const every = minutes === 1 ? "every minute" : `every ${minutes} minutes`
  const lines: { text: string; mono?: boolean }[] = []
  if (refused) {
    lines.push({
      text: `The forge refused the webhook. Ariadne checks for changes ${every}, and tries the webhook again each time.`,
    })
    lines.push({
      text: "A webhook needs two things: admin rights on the repository, and the admin:repo_hook scope on the gh token.",
    })
    lines.push({ text: "To add the scope, run:" })
    lines.push({ text: `gh auth refresh -h ${forge.host} -s admin:repo_hook`, mono: true })
    lines.push({ text: "Turn off the webhook for this repository to stop the retries." })
    if (webhook.error) lines.push({ text: `Webhook: ${webhook.error}` })
    if (webhook.fetch_error) lines.push({ text: `Last fetch: ${webhook.fetch_error}` })
  } else if (!forge.webhook_enabled && working) {
    lines.push({ text: "The webhook is off for this repository." })
    lines.push({ text: `Ariadne checks the forge for changes ${every}.` })
  } else if (mode === "smee" && working) {
    lines.push({
      text: "The webhook is live: changes arrive the moment they happen.",
    })
    if (webhook.url) lines.push({ text: webhook.url, mono: true })
  } else if (mode === "smee") {
    lines.push({
      text: `The webhook is not working. Ariadne checks the forge ${every} until it works.`,
    })
    if (tunnel?.state === "down" && tunnel.error) {
      lines.push({ text: `Tunnel: ${tunnel.error}` })
    }
    if (webhook.error) lines.push({ text: `Webhook: ${webhook.error}` })
    if (webhook.fetch_error) lines.push({ text: `Last fetch: ${webhook.fetch_error}` })
  } else if (working) {
    lines.push({ text: `Ariadne checks the forge for changes ${every}.` })
    lines.push({
      text: "Turn on the webhook tunnel in Settings (the gear in the header) to get them the moment they happen.",
    })
  } else {
    lines.push({
      text: `Ariadne could not read the forge. It tries again ${every}.`,
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
