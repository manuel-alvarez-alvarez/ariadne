/**
 * The one source of datasources this app needs: no project scoping, no
 * datasource CRUD of its own — just the daemon's own Prometheus and Loki
 * APIs, each a single global datasource with a `directUrl` of the daemon
 * origin it was opened with (rule 2: the daemon serves `/api/v1/*` and
 * `/loki/api/v1/*` at its own root).
 */

import type {
  DatasourceApi,
  DatasourceResource,
  DatasourceSelector,
  GlobalDatasourceResource,
} from "@perses-dev/client"

const PROMETHEUS_KIND = "PrometheusDatasource"
const LOKI_KIND = "LokiDatasource"

function globalDatasource(name: string, kind: string, directUrl: string): GlobalDatasourceResource {
  return {
    kind: "GlobalDatasource",
    metadata: { name },
    spec: { default: true, plugin: { kind, spec: { directUrl } } },
  }
}

/**
 * The app's one `DatasourceApi`: two global datasources, nothing
 * project-scoped. Both query `/v1/hosts/{host}/` of the daemon it was opened
 * with (037, rules 16 to 19): a host name, or `all` for every host at once.
 */
export function createDatasourceApi(daemonUrl: string, host = "local"): DatasourceApi {
  const directUrl = `${daemonUrl}/v1/hosts/${encodeURIComponent(host)}`
  const datasources = [
    globalDatasource("prometheus", PROMETHEUS_KIND, directUrl),
    globalDatasource("loki", LOKI_KIND, directUrl),
  ]

  return {
    async getDatasource(): Promise<DatasourceResource | undefined> {
      return undefined
    },
    async getGlobalDatasource(
      selector: DatasourceSelector,
    ): Promise<GlobalDatasourceResource | undefined> {
      return datasources.find((datasource) => datasource.spec.plugin.kind === selector.kind)
    },
    async listDatasources(): Promise<DatasourceResource[]> {
      return []
    },
    async listGlobalDatasources(pluginKind?: string): Promise<GlobalDatasourceResource[]> {
      return pluginKind
        ? datasources.filter((datasource) => datasource.spec.plugin.kind === pluginKind)
        : datasources
    },
  }
}
