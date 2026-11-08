import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import App from "./App"
import { markPlatform } from "./lib/platform"
import "./index.css"

markPlatform(document.documentElement, navigator.userAgent)

const container = document.getElementById("root")
if (!container) throw new Error("missing #root element")

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
