/**
 * What a step's detail asks of the console around it: the full view, which
 * shows a step whole and keeps the shown one at its newest version, and the
 * live steps of the subagents. Without a full view around it, a detail
 * offers no `Open`.
 */

import { createContext, useContext } from "react"

import type { StepDto } from "@/api"

export interface FullView {
  /** Show `step` whole. The keyboard goes back to `trigger` when the view closes. */
  show: (step: StepDto, trigger?: HTMLElement | null) => void
}

export const FullViewContext = createContext<FullView | null>(null)

/** The live steps of the subagents' lanes, by step id: what the stream brought last. */
export const ChildStepsContext = createContext<Readonly<Record<string, StepDto>>>({})

export function useChildSteps(): Readonly<Record<string, StepDto>> {
  return useContext(ChildStepsContext)
}

export function useFullView(): FullView | null {
  return useContext(FullViewContext)
}
