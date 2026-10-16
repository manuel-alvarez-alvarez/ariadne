import { describe, expect, it } from "vitest"
import { ALLOWLIST, decide } from "./check-audit.mjs"

const ALLOWED_ADVISORY = ALLOWLIST[0].id

function vuln(name, severity, via) {
  return { name, severity, via }
}

describe("decide", () => {
  it("passes a report with no high or critical finding", () => {
    expect(decide({ vulnerabilities: { hono: vuln("hono", "moderate", ["x"]) } })).toBe(true)
  })

  it("passes exactly the documented braces chain", () => {
    const report = {
      vulnerabilities: {
        braces: vuln("braces", "high", [
          { url: `https://github.com/advisories/${ALLOWED_ADVISORY}` },
        ]),
        micromatch: vuln("micromatch", "high", ["braces"]),
        "fast-glob": vuln("fast-glob", "high", ["micromatch"]),
        "@ts-morph/common": vuln("@ts-morph/common", "high", ["fast-glob"]),
        "ts-morph": vuln("ts-morph", "high", ["@ts-morph/common"]),
        shadcn: vuln("shadcn", "high", ["fast-glob", "ts-morph"]),
      },
    }
    expect(decide(report)).toBe(true)
  })

  it("fails a new high finding in a package outside the allowlist", () => {
    const report = {
      vulnerabilities: {
        braces: vuln("braces", "high", [
          { url: `https://github.com/advisories/${ALLOWED_ADVISORY}` },
        ]),
        "left-pad": vuln("left-pad", "high", [
          { url: "https://github.com/advisories/GHSA-0000-0000-0000" },
        ]),
      },
    }
    expect(decide(report)).toBe(false)
  })

  it("fails a different advisory on the allowlisted package", () => {
    const report = {
      vulnerabilities: {
        braces: vuln("braces", "high", [
          { url: "https://github.com/advisories/GHSA-1111-1111-1111" },
        ]),
      },
    }
    expect(decide(report)).toBe(false)
  })

  it("fails a critical finding the same way", () => {
    const report = {
      vulnerabilities: {
        "left-pad": vuln("left-pad", "critical", [
          { url: "https://github.com/advisories/GHSA-0000-0000-0000" },
        ]),
      },
    }
    expect(decide(report)).toBe(false)
  })

  it("fails a high chain whose root advisory is missing from the report", () => {
    const report = {
      vulnerabilities: {
        micromatch: vuln("micromatch", "high", ["braces"]),
      },
    }
    expect(decide(report)).toBe(false)
  })

  it("fails an error report with no vulnerabilities object, such as a registry outage", () => {
    const report = { error: { code: "ETIMEDOUT", summary: "request to the registry timed out" } }
    expect(decide(report)).toBe(false)
  })
})
