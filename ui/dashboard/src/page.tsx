/**
 * The whole app: one Perses dashboard, in `ViewDashboard`, over the daemon
 * named on this page's own query string (rule 2). `ViewDashboard` opens in
 * view mode — its own Edit button, not a prop here, is what switches it into
 * edit mode; Save and Cancel, which Perses shows once there, both land back
 * in view mode on their own.
 *
 * The provider tree below `ViewDashboard` itself is Perses's own choice, not
 * ours — `ViewDashboard` wraps its datasource, time-range and variable state
 * internally. What a host still supplies is everything `ViewDashboard` reads
 * out of context rather than props: a `@tanstack/react-query` v4 client (a
 * second one from the host app's own v5, since this is a separate React
 * tree entirely), the MUI theme and the matching Perses charts theme, a
 * snackbar host for its own toasts, the `use-query-params` provider its
 * internal `*WithQueryParams` hooks read and write the URL through — the
 * window adapter, since there is no router here to hand it instead — the
 * plugin registry, and the `ValidationProvider` whose schemas its panel,
 * variable and datasource editors read.
 *
 * Perses resources are project-scoped; the daemon's dashboards are not, so
 * `PROJECT` is a constant this page invents to satisfy that shape and strips
 * back out before a save reaches the daemon.
 */

import { CssBaseline } from "@mui/material"
import { ThemeProvider } from "@mui/material/styles"
import type { DashboardResource } from "@perses-dev/client"
import {
  ChartsProvider,
  generateChartsTheme,
  getTheme,
  SnackbarProvider,
} from "@perses-dev/components"
import { ViewDashboard } from "@perses-dev/dashboards"
import { PluginRegistry, ValidationProvider } from "@perses-dev/plugin-system"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { QueryParamProvider } from "use-query-params"
import { WindowHistoryAdapter } from "use-query-params/adapters/window"

import { getDashboard, saveDashboard } from "./dashboard-client"
import { createDatasourceApi } from "./datasource-api"
import { readPageParams } from "./page-params"
import { pluginLoader } from "./plugins"
import { useHostMessages } from "./use-host-messages"

const queryClient = new QueryClient()

const PROJECT = "ariadne"

function toResource(fallbackName: string, document: unknown): DashboardResource {
  const doc = document as { kind?: string; metadata?: { name?: string }; spec?: unknown }
  return {
    kind: doc.kind === "EphemeralDashboard" ? "EphemeralDashboard" : "Dashboard",
    metadata: { name: doc.metadata?.name ?? fallbackName, project: PROJECT },
    spec: (doc.spec ?? {}) as DashboardResource["spec"],
  }
}

/** The inverse of {@link toResource}: drops `project`, which the daemon's document never carried. */
function fromResource(resource: DashboardResource): unknown {
  return { kind: resource.kind, metadata: { name: resource.metadata.name }, spec: resource.spec }
}

export function Page() {
  const [params] = useState(() => readPageParams(window.location.search))
  const [theme, setTheme] = useState(params.theme)
  const [dashboard, setDashboard] = useState<DashboardResource | null>(null)
  const [error, setError] = useState<unknown>(null)
  // `version` forces `ViewDashboard` to remount — its own internal store
  // only ever reads `dashboardResource` as an initial value, so a reload
  // that only changes the prop leaves it showing what it had.
  const [version, setVersion] = useState(0)
  const mounted = useRef(true)
  useEffect(() => {
    // StrictMode mounts, unmounts and remounts once in development to
    // surface an effect that cleaned up wrong; the flag has to come back up
    // on that remount; `useRef(true)`'s initial value only ever runs once.
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const datasourceApi = useMemo(() => createDatasourceApi(params.daemonUrl), [params.daemonUrl])
  const muiTheme = useMemo(() => getTheme(theme, undefined, true), [theme])
  const chartsTheme = useMemo(() => generateChartsTheme(muiTheme, {}), [muiTheme])

  const load = useCallback(() => {
    setError(null)
    getDashboard(params.daemonUrl, params.dashboardName)
      .then((document) => {
        if (!mounted.current) return
        setDashboard(toResource(params.dashboardName, document))
        setVersion((v) => v + 1)
      })
      .catch((cause: unknown) => {
        if (mounted.current) setError(cause)
      })
  }, [params.daemonUrl, params.dashboardName])

  useEffect(() => {
    load()
  }, [load])

  useHostMessages({
    onTheme: (next) => setTheme(next === "dark" ? "dark" : "light"),
    onReload: load,
  })

  async function onSave(resource: DashboardResource): Promise<unknown> {
    const saved = await saveDashboard(
      params.daemonUrl,
      params.dashboardName,
      fromResource(resource),
    )
    const next = toResource(params.dashboardName, saved)
    setDashboard(next)
    return next
  }

  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider theme={muiTheme}>
        <CssBaseline />
        <ChartsProvider chartsTheme={chartsTheme}>
          <SnackbarProvider anchorOrigin={{ vertical: "bottom", horizontal: "right" }}>
            <QueryParamProvider adapter={WindowHistoryAdapter}>
              <PluginRegistry
                pluginLoader={pluginLoader}
                defaultPluginKinds={{ TimeSeriesQuery: "PrometheusTimeSeriesQuery" }}
              >
                {error ? (
                  <p role="alert">
                    Could not load {params.dashboardName}: {String(error)}
                  </p>
                ) : dashboard ? (
                  <ValidationProvider>
                    <ViewDashboard
                      key={version}
                      dashboardResource={dashboard}
                      datasourceApi={datasourceApi}
                      isReadonly={false}
                      isVariableEnabled
                      isAnnotationEnabled={false}
                      isDatasourceEnabled={false}
                      onSave={onSave}
                    />
                  </ValidationProvider>
                ) : (
                  <p>Loading…</p>
                )}
              </PluginRegistry>
            </QueryParamProvider>
          </SnackbarProvider>
        </ChartsProvider>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
