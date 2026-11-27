/**
 * The slot for the detail of one step, under its row in an open activity
 * fold. A later task draws the detail by the kind of the step, and opens it
 * in place. Until then the slot draws nothing.
 */

import type { Host, StepDto } from "@/api"

export function StepDetail(_props: { host: Host; sessionId: string; step: StepDto }) {
  return null
}
