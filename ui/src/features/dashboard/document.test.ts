import { describe, expect, it } from "vitest"

import { copyDashboardDocument, emptyDashboardDocument } from "./document"

describe("emptyDashboardDocument", () => {
  it("is a valid, empty Perses dashboard named for the row the store will file it under", () => {
    expect(emptyDashboardDocument("mine")).toEqual({
      kind: "Dashboard",
      metadata: { name: "mine" },
      spec: {},
    })
  })
})

describe("copyDashboardDocument", () => {
  const source = {
    kind: "Dashboard",
    metadata: { name: "stats" },
    spec: {
      display: { name: "Ariadne stats" },
      duration: "7d",
      variables: [{ kind: "ListVariable", spec: { name: "repo" } }],
      layouts: [{ kind: "Grid", spec: { items: [] } }],
      panels: { usage: { kind: "Panel" } },
    },
  }

  it("carries the panels, layouts, variables and time settings under the new name", () => {
    expect(copyDashboardDocument(source, "mine")).toEqual({
      kind: "Dashboard",
      metadata: { name: "mine" },
      spec: source.spec,
    })
  })

  it("never writes the new name back into the document it was copied from", () => {
    copyDashboardDocument(source, "mine")

    expect(source.metadata.name).toBe("stats")
  })

  it("clones the spec rather than aliasing it, so editing the copy leaves the source untouched", () => {
    const copy = copyDashboardDocument(source, "mine") as typeof source

    copy.spec.duration = "1d"

    expect(source.spec.duration).toBe("7d")
  })
})
