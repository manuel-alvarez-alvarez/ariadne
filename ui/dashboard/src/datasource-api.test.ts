import { describe, expect, it } from "vitest"

import { createDatasourceApi } from "./datasource-api"

const DAEMON_URL = "http://127.0.0.1:7676"
const LOCAL_URL = `${DAEMON_URL}/v1/hosts/local`

describe("createDatasourceApi", () => {
  it("answers a global Prometheus datasource with the daemon's local host as its direct URL", async () => {
    const api = createDatasourceApi(DAEMON_URL)
    const datasource = await api.getGlobalDatasource({ kind: "PrometheusDatasource" })
    expect(datasource?.kind).toBe("GlobalDatasource")
    expect(datasource?.spec.plugin).toEqual({
      kind: "PrometheusDatasource",
      spec: { directUrl: LOCAL_URL },
    })
  })

  it("answers a global Loki datasource with the daemon's local host as its direct URL", async () => {
    const api = createDatasourceApi(DAEMON_URL)
    const datasource = await api.getGlobalDatasource({ kind: "LokiDatasource" })
    expect(datasource?.spec.plugin).toEqual({
      kind: "LokiDatasource",
      spec: { directUrl: LOCAL_URL },
    })
  })

  it("answers nothing for a kind it was not given", async () => {
    const api = createDatasourceApi(DAEMON_URL)
    expect(await api.getGlobalDatasource({ kind: "TempoDatasource" })).toBeUndefined()
  })

  it("lists both global datasources unfiltered, and one kind filtered", async () => {
    const api = createDatasourceApi(DAEMON_URL)
    expect((await api.listGlobalDatasources()).map((d) => d.metadata.name)).toEqual([
      "prometheus",
      "loki",
    ])
    expect((await api.listGlobalDatasources("LokiDatasource")).map((d) => d.metadata.name)).toEqual(
      ["loki"],
    )
  })

  it("has no project-scoped datasource of its own", async () => {
    const api = createDatasourceApi(DAEMON_URL)
    expect(await api.getDatasource("any", { kind: "PrometheusDatasource" })).toBeUndefined()
    expect(await api.listDatasources("any")).toEqual([])
  })

  it("points both datasources at the named host, or at all of them", async () => {
    for (const host of ["box", "all"]) {
      const api = createDatasourceApi(DAEMON_URL, host)
      const directUrls = (await api.listGlobalDatasources()).map(
        (d) => (d.spec.plugin.spec as { directUrl: string }).directUrl,
      )
      expect(directUrls).toEqual([
        `${DAEMON_URL}/v1/hosts/${host}`,
        `${DAEMON_URL}/v1/hosts/${host}`,
      ])
    }
  })
})
