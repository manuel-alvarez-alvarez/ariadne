/**
 * The two daemon calls this app makes on its own: load a dashboard's
 * document, and save one over it (rule 2). Everything else a dashboard reads
 * — its metrics, its logs — goes straight from Perses to the daemon's own
 * Prometheus and Loki APIs, through `datasource-api.ts`.
 *
 * Hand-rolled rather than through the host app's generated `openapi-fetch`
 * client: this package is its own React 18 project, built and shipped apart
 * from `ui/`'s, and two endpoints are not worth a second copy of its codegen
 * pipeline.
 */

/** The envelope every daemon error carries. */
interface ErrorEnvelope {
  error?: { code?: string; message?: string }
}

async function unwrap(response: Response, verb: string, path: string): Promise<unknown> {
  if (response.ok) return response.json()
  const body = (await response.json().catch(() => undefined)) as ErrorEnvelope | undefined
  const message = body?.error?.message ?? response.statusText
  throw new Error(`${verb} ${path} -> ${response.status} ${message}`)
}

/** The raw Perses document of one dashboard, read off its `DashboardDto`. */
export async function getDashboard(daemonUrl: string, name: string): Promise<unknown> {
  const path = `/v1/dashboards/${encodeURIComponent(name)}`
  const response = await fetch(`${daemonUrl}${path}`)
  const dto = (await unwrap(response, "GET", path)) as { document: unknown }
  return dto.document
}

/** Saves a document over a dashboard's, and returns what the daemon kept. */
export async function saveDashboard(
  daemonUrl: string,
  name: string,
  document: unknown,
): Promise<unknown> {
  const path = `/v1/dashboards/${encodeURIComponent(name)}`
  const response = await fetch(`${daemonUrl}${path}`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ document }),
  })
  const dto = (await unwrap(response, "PUT", path)) as { document: unknown }
  return dto.document
}
