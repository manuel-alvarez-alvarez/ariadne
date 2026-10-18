/**
 * Every plugin kind the dashboard opens with: the two datasources, the nine
 * panel types, and the two variable kinds (rule 1). Each package ships its
 * own `getPluginModule()`, read off its `package.json`'s `perses.plugins`
 * manifest; `dynamicImportPluginLoader` is what `PluginRegistry` expects in
 * return, and is the one the task names, over a loader of our own.
 */

import * as barChartPlugin from "@perses-dev/bar-chart-plugin"
import * as datasourceVariablePlugin from "@perses-dev/datasource-variable-plugin"
import * as gaugeChartPlugin from "@perses-dev/gauge-chart-plugin"
import * as logsTablePlugin from "@perses-dev/logs-table-plugin"
import * as lokiPlugin from "@perses-dev/loki-plugin"
import * as markdownPlugin from "@perses-dev/markdown-plugin"
import * as pieChartPlugin from "@perses-dev/pie-chart-plugin"
import type { DynamicImportPlugin, PluginLoader } from "@perses-dev/plugin-system"
import { dynamicImportPluginLoader, getPluginModuleCompoundKey } from "@perses-dev/plugin-system"
import * as prometheusPlugin from "@perses-dev/prometheus-plugin"
import * as statChartPlugin from "@perses-dev/stat-chart-plugin"
import * as staticListVariablePlugin from "@perses-dev/static-list-variable-plugin"
import * as tablePlugin from "@perses-dev/table-plugin"
import * as timeseriesChartPlugin from "@perses-dev/timeseries-chart-plugin"
import * as timeseriesTablePlugin from "@perses-dev/timeseries-table-plugin"

/** A module whose own package carries a `getPluginModule` export. */
interface PluginModule {
  getPluginModule: () => DynamicImportPlugin["resource"]
}

/**
 * `PluginRegistry` reads a loaded plugin off the module under its compound
 * key (`Panel:StatChart::0.14.0`), not under the export's own name, so the
 * module it is handed maps each plugin of the manifest to that key.
 */
function plugin(module: PluginModule): DynamicImportPlugin {
  const resource = module.getPluginModule()
  const { registry, version } = resource.metadata
  const exports = module as unknown as Record<string, unknown>
  const keyed = Object.fromEntries(
    resource.spec.plugins.map(({ kind, spec: { name } }) => [
      getPluginModuleCompoundKey({ kind, name, registry, version }),
      exports[name],
    ]),
  )
  return { resource, importPlugin: () => Promise.resolve(keyed) }
}

export const pluginLoader: PluginLoader = dynamicImportPluginLoader([
  plugin(prometheusPlugin),
  plugin(lokiPlugin),
  plugin(timeseriesChartPlugin),
  plugin(statChartPlugin),
  plugin(barChartPlugin),
  plugin(pieChartPlugin),
  plugin(gaugeChartPlugin),
  plugin(tablePlugin),
  plugin(timeseriesTablePlugin),
  plugin(logsTablePlugin),
  plugin(markdownPlugin),
  plugin(staticListVariablePlugin),
  plugin(datasourceVariablePlugin),
])
