/**
 * What a step's detail asks of the full view around it: show a step whole,
 * and keep the shown one at its newest version. Without a full view around
 * it, a detail offers no `Open`.
 */

import { createContext, useContext } from "react"

import type { StepDto } from "@/api"

export interface FullView {
  /** The id of the step the full view shows, if any. */
  shownId?: string
  /** Show `step` whole. The keyboard goes back to `trigger` when the view closes. */
  show: (step: StepDto, trigger?: HTMLElement | null) => void
  /** Keep the shown step at its newest version: a running call's progress. */
  refresh: (step: StepDto) => void
}

export const FullViewContext = createContext<FullView | null>(null)

export function useFullView(): FullView | null {
  return useContext(FullViewContext)
}
