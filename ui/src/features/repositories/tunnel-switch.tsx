/**
 * The webhook tunnel in the repositories screen's header: its state, and the
 * switch that turns it on or off (027).
 *
 * Off means every enabled repository fetches on its timer; the hooks stay
 * registered. The state follows `forge_settings_updated`, so a tunnel that
 * drops or comes back shows here without a refetch.
 */

import { useQuery } from "@tanstack/react-query"
import { toast } from "sonner"

import type { ForgeTunnelDto } from "@/api"
import { Switch } from "@/components/ui/switch"
import { describeError } from "@/lib/format"

import { tunnelQueryOptions, useSetTunnel } from "./queries"

const STATE_LABELS: Record<ForgeTunnelDto["state"], string> = {
  up: "Tunnel up",
  down: "Tunnel down",
  off: "Tunnel off",
}

/** What the state says beside the switch, and what its tooltip adds. */
function details(tunnel: ForgeTunnelDto): string {
  return [
    tunnel.url ? `URL ${tunnel.url}` : null,
    tunnel.listen ? `forwards to ${tunnel.listen}` : null,
    `since ${tunnel.since}`,
    tunnel.error ? `error: ${tunnel.error}` : null,
  ]
    .filter(Boolean)
    .join(" · ")
}

export function TunnelSwitch() {
  const tunnel = useQuery(tunnelQueryOptions())
  const update = useSetTunnel()
  if (!tunnel.data) return null
  const current = tunnel.data
  return (
    <div className="flex items-center gap-2 text-xs" title={details(current)}>
      <span className={current.state === "up" ? "" : "text-muted-foreground"}>
        {STATE_LABELS[current.state]}
      </span>
      <Switch
        aria-label="Webhook tunnel"
        checked={current.enabled}
        disabled={update.isPending}
        onCheckedChange={(enabled) =>
          update.mutate(
            { enabled },
            {
              onError: (error) =>
                toast.error(
                  enabled ? "Could not turn the tunnel on" : "Could not turn the tunnel off",
                  { description: describeError(error) },
                ),
            },
          )
        }
      />
    </div>
  )
}
