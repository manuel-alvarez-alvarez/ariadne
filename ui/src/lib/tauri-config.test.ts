import { describe, expect, it } from "vitest"
import configText from "../../src-tauri/tauri.conf.json?raw"

const config = JSON.parse(configText) as {
  app: {
    windows: Array<{ acceptFirstMouse?: boolean }>
    security: { csp: string }
  }
}

describe("Tauri window configuration", () => {
  it("lets the first click operate a control on an inactive window", () => {
    expect(config.app.windows).toHaveLength(1)
    expect(config.app.windows[0]?.acceptFirstMouse).toBe(true)
  })

  it("carries a restrictive content security policy", () => {
    const { csp } = config.app.security

    expect(csp).toContain("script-src 'self'")
    expect(csp).toContain("object-src 'none'")
    expect(csp).toContain("base-uri 'none'")
    expect(csp).toContain("frame-ancestors 'none'")
    expect(csp).toContain("connect-src 'self' http: https: ws: wss:")
    expect(csp).not.toContain("unsafe-eval")
  })
})
