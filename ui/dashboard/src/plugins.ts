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
import { dynamicImportPluginLoader } from "@perses-dev/plugin-system"
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

function plugin(module: PluginModule): DynamicImportPlugin {
  return { resource: module.getPluginModule(), importPlugin: () => Promise.resolve(module) }
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
