import { createContext } from "react"

export const InspectorLayoutContext = createContext<{
  reserve: (width: number) => void
} | null>(null)
