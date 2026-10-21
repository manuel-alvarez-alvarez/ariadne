import { isTauri } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { type ReactNode, useEffect } from "react"

import { useBaseUrl, useDiagnosticsEnabled } from "@/stores/settings"
import { configureTelemetry, flushTelemetry, recordLog } from "@/telemetry/telemetry"

/** Configures OTLP and mirrors browser errors while diagnostics are enabled. */
export function TelemetryProvider({ children }: { children: ReactNode }) {
  const baseUrl = useBaseUrl()
  const enabled = useDiagnosticsEnabled()

  useEffect(() => {
    configureTelemetry({ baseUrl, enabled })
    return () => void flushTelemetry()
  }, [baseUrl, enabled])

  useEffect(() => {
    if (!enabled) return
    const warn = console.warn
    const error = console.error
    console.warn = (...args: unknown[]) => {
      const entry = consoleEntry(args)
      recordLog("warn", entry.message, entry.stack)
      warn(...args)
    }
    console.error = (...args: unknown[]) => {
      const entry = consoleEntry(args)
      recordLog("error", entry.message, entry.stack)
      error(...args)
    }
    const onError = (event: ErrorEvent) => recordLog("error", event.message, event.error?.stack)
    const onRejection = (event: PromiseRejectionEvent) => {
      const reason = event.reason
      recordLog("error", reason instanceof Error ? reason.message : String(reason), reason?.stack)
    }
    window.addEventListener("error", onError)
    window.addEventListener("unhandledrejection", onRejection)
    return () => {
      console.warn = warn
      console.error = error
      window.removeEventListener("error", onError)
      window.removeEventListener("unhandledrejection", onRejection)
    }
  }, [enabled])

  useEffect(() => {
    if (!enabled || !isTauri()) return
    let active = true
    let unlisten: (() => void) | undefined
    void listen<{ level: number; message: string }>("log://log", ({ payload }) => {
      const severity = ({ 1: "trace", 2: "debug", 3: "info", 4: "warn", 5: "error" } as const)[
        payload.level as 1 | 2 | 3 | 4 | 5
      ]
      if (severity) recordLog(severity, payload.message, { target: "tauri" })
    }).then((stop) => {
      if (active) unlisten = stop
      else stop()
    })
    return () => {
      active = false
      unlisten?.()
    }
  }, [enabled])

  return children
}

function consoleEntry(args: unknown[]): { message: string; stack?: string } {
  const first = args[0]
  if (first instanceof Error) return { message: first.message, stack: first.stack }
  const error = args.find((arg): arg is Error => arg instanceof Error)
  return { message: String(first ?? ""), stack: error?.stack }
}
