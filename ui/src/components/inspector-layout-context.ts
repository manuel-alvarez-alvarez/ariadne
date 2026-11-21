import { createContext } from "react"

export const InspectorLayoutContext = createContext<{
  focusSlot?: HTMLElement | null
  reserve: (width: number) => void
  focusOverview: () => void
} | null>(null)
