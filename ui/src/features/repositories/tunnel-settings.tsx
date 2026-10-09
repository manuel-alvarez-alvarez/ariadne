/**
 * The webhook tunnel, as the settings dialog shows it (027): what it is for,
 * the switch that turns it on or off, and the state it is in.
 *
 * Off means every enabled repository fetches from its forge on a timer; the
 * hooks stay registered. The state follows `forge_settings_updated`, so a
 * tunnel that drops or comes back shows here without a refetch.
 */

import { useQuery } from "@tanstack/react-query"
import { toast } from "sonner"

import type { ForgeTunnelDto } from "@/api"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Switch } from "@/components/ui/switch"
import { cn, describeError } from "@/lib/format"

import { tunnelQueryOptions, useSetTunnel } from "./queries"

const STATES: Record<ForgeTunnelDto["state"], { label: string; dot: string }> = {
  up: { label: "Tunnel up", dot: "bg-status-done" },
  down: { label: "Tunnel down", dot: "bg-status-danger" },
  off: { label: "Tunnel off", dot: "bg-muted-foreground" },
}

/** The one line of detail the state carries: where it points, or why it failed. */
function detail(tunnel: ForgeTunnelDto): string | null {
  if (tunnel.state === "up") return tunnel.url ?? null
  if (tunnel.state === "down") return tunnel.error ?? null
  // On but `off` is the daemon declining to open one.
  return tunnel.enabled
    ? "Not needed: a public webhook URL is configured, or no repository has its forge enabled"
    : null
}

export function TunnelSettings({ open }: { open: boolean }) {
  const tunnel = useQuery({ ...tunnelQueryOptions(), enabled: open })
  const update = useSetTunnel()
  const current = tunnel.data
  return (
    <Field>
      <div className="flex items-center justify-between gap-3">
        <FieldLabel htmlFor="webhook-tunnel">Webhook tunnel</FieldLabel>
        <Switch
          id="webhook-tunnel"
          aria-label="Webhook tunnel"
          checked={current?.enabled ?? false}
          disabled={!current || update.isPending}
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
      <FieldDescription>
        Opens a public URL that forwards to the daemon, so GitHub and GitLab can push pull request
        and issue changes the moment they happen. Off, each repository with its forge enabled checks
        for changes every 5 minutes instead. Applies at once.
      </FieldDescription>
      {current ? <TunnelState tunnel={current} /> : null}
      {tunnel.isError ? (
        <p className="text-xs text-destructive">
          Could not read the tunnel: {describeError(tunnel.error)}
        </p>
      ) : null}
    </Field>
  )
}

function TunnelState({ tunnel }: { tunnel: ForgeTunnelDto }) {
  const state = STATES[tunnel.state]
  const more = detail(tunnel)
  return (
    <p className="flex min-w-0 items-center gap-2 text-xs" title={`Since ${tunnel.since}`}>
      <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-full", state.dot)} />
      <span className="shrink-0">{state.label}</span>
      {more ? <span className="truncate font-mono text-muted-foreground">{more}</span> : null}
    </p>
  )
}
