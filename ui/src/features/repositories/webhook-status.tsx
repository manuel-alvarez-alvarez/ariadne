import type { ForgeDto } from "@/api"

/** The same read-only facts in the table, form, and command-line inspection. */
export function WebhookStatus({ webhook }: { webhook: ForgeDto["webhook"] }) {
  return (
    <dl className="grid gap-1 text-xs break-all">
      <div>
        <dt className="text-muted-foreground">Webhook state</dt>
        <dd>{webhook.state}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Webhook URL</dt>
        <dd>{webhook.url ?? "-"}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Webhook error</dt>
        <dd>{webhook.error ?? "-"}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">Last delivery</dt>
        <dd>{webhook.last_delivery_at ?? "-"}</dd>
      </div>
    </dl>
  )
}
