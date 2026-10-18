import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import { Page } from "./page"

const root = document.getElementById("root")
if (!root) throw new Error("#root is missing from index.html")

createRoot(root).render(
  <StrictMode>
    <Page />
  </StrictMode>,
)
