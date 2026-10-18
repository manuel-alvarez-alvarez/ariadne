// @vitest-environment jsdom

/**
 * `PluginRegistry` finds a loaded plugin on its module under the plugin's
 * compound key (`Panel:StatChart::0.14.0`), not under the export's own
 * name: a module handed back as the package exports it leaves every panel
 * reading "A StatChart plugin for kind 'Panel' is not installed".
 *
 * The real plugin packages are mocked: outside Vite's bundler they fail to
 * load (a `../package.json` require, font CSS). Each stand-in has the shape
 * of the real one, a `getPluginModule()` manifest beside an export named
 * after each plugin.
 */

import { getPluginModuleCompoundKey } from "@perses-dev/plugin-system"
import { describe, expect, it, vi } from "vitest"

const { fake } = vi.hoisted(() => ({
  fake: (kind: string, ...names: string[]) => ({
    getPluginModule: () => ({
      kind: "PluginModule",
      metadata: { name: `@perses-dev/${names[0]}`, version: "1.0.0" },
      spec: { plugins: names.map((name) => ({ kind, spec: { name, display: { name } } })) },
    }),
    ...Object.fromEntries(names.map((name) => [name, { plugin: name }])),
    Unrelated: { plugin: "not a plugin" },
  }),
}))

vi.mock("@perses-dev/prometheus-plugin", () => fake("Datasource", "PrometheusDatasource"))
vi.mock("@perses-dev/loki-plugin", () => fake("Datasource", "LokiDatasource"))
vi.mock("@perses-dev/timeseries-chart-plugin", () => fake("Panel", "TimeSeriesChart"))
vi.mock("@perses-dev/stat-chart-plugin", () => fake("Panel", "StatChart"))
vi.mock("@perses-dev/bar-chart-plugin", () => fake("Panel", "BarChart"))
vi.mock("@perses-dev/pie-chart-plugin", () => fake("Panel", "PieChart"))
vi.mock("@perses-dev/gauge-chart-plugin", () => fake("Panel", "GaugeChart"))
vi.mock("@perses-dev/table-plugin", () => fake("Panel", "Table"))
vi.mock("@perses-dev/timeseries-table-plugin", () => fake("Panel", "TimeSeriesTable"))
vi.mock("@perses-dev/logs-table-plugin", () => fake("Panel", "LogsTable"))
vi.mock("@perses-dev/markdown-plugin", () => fake("Panel", "Markdown"))
vi.mock("@perses-dev/static-list-variable-plugin", () => fake("Variable", "StaticListVariable"))
vi.mock("@perses-dev/datasource-variable-plugin", () => fake("Variable", "DatasourceVariable"))

import { pluginLoader } from "./plugins"

describe("pluginLoader", () => {
  it("hands back each plugin under the compound key the registry reads", async () => {
    const stat = (await pluginLoader.getInstalledPlugins()).find(
      (resource) => resource.metadata.name === "@perses-dev/StatChart",
    )
    if (stat === undefined) throw new Error("StatChart is not installed")
    const module = (await pluginLoader.importPluginModule(stat)) as Record<string, unknown>

    expect(module).toEqual({
      [getPluginModuleCompoundKey({ kind: "Panel", name: "StatChart", version: "1.0.0" })]: {
        plugin: "StatChart",
      },
    })
  })

  it("loads every installed plugin, the logs table included", async () => {
    const missing: string[] = []
    for (const resource of await pluginLoader.getInstalledPlugins()) {
      const { registry, version } = resource.metadata
      const module = (await pluginLoader.importPluginModule(resource)) as Record<string, unknown>
      for (const { kind, spec } of resource.spec.plugins) {
        const key = getPluginModuleCompoundKey({ kind, name: spec.name, registry, version })
        if (!module[key]) missing.push(key)
      }
    }
    expect(missing).toEqual([])
  })
})
