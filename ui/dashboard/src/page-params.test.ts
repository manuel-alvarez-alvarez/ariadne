import { describe, expect, it } from "vitest"

import { readPageParams } from "./page-params"

describe("readPageParams", () => {
  it("reads the daemon, the dashboard and the theme off the query string", () => {
    expect(readPageParams("?daemon=http://127.0.0.1:7676&dashboard=stats&theme=dark")).toEqual({
      daemonUrl: "http://127.0.0.1:7676",
      dashboardName: "stats",
      theme: "dark",
    })
  })

  it("defaults the theme to light on anything but exactly dark", () => {
    expect(readPageParams("?theme=light").theme).toBe("light")
    expect(readPageParams("?theme=sepia").theme).toBe("light")
    expect(readPageParams("").theme).toBe("light")
  })

  it("defaults the daemon and the dashboard name to the empty string when absent", () => {
    expect(readPageParams("")).toEqual({ daemonUrl: "", dashboardName: "", theme: "light" })
  })
})
