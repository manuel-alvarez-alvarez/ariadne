import { describe, expect, it } from "vitest"
import configText from "../../src-tauri/tauri.conf.json?raw"

const config = JSON.parse(configText) as {
  app: { windows: Array<{ acceptFirstMouse?: boolean }> }
}

describe("Tauri window configuration", () => {
  it("lets the first click operate a control on an inactive window", () => {
    expect(config.app.windows).toHaveLength(1)
    expect(config.app.windows[0]?.acceptFirstMouse).toBe(true)
  })
})
