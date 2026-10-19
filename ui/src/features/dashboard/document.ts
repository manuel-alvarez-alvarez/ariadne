/**
 * What a new dashboard's document starts from: nothing, or everything the
 * picked one already has, under the new name the store's own validation
 * requires `metadata.name` to carry (036).
 */

/** The shape every dashboard document carries; `spec` is opaque to the host. */
interface DashboardDocument {
  kind: "Dashboard"
  metadata: { name: string; [key: string]: unknown }
  [key: string]: unknown
}

/** A valid, empty Perses dashboard — the store accepts an object `spec`. */
export function emptyDashboardDocument(name: string): DashboardDocument {
  return { kind: "Dashboard", metadata: { name }, spec: {} }
}

/**
 * The picked dashboard's panels, layouts, variables and time settings, under
 * a new name. A deep clone, so editing the copy never writes back into the
 * document it was copied from.
 */
export function copyDashboardDocument(document: unknown, name: string): DashboardDocument {
  const cloned = structuredClone(document) as DashboardDocument
  cloned.metadata = { ...cloned.metadata, name }
  const spec = cloned.spec
  if (typeof spec === "object" && spec !== null && !Array.isArray(spec)) {
    const display = (spec as { display?: unknown }).display
    if (typeof display === "object" && display !== null && !Array.isArray(display)) {
      cloned.spec = { ...spec, display: { ...display, name } }
    }
  }
  return cloned
}
