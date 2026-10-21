import { context, DiagLogLevel, diag } from "@opentelemetry/api"
import { SeverityNumber } from "@opentelemetry/api-logs"
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-http/build/esm/platform/browser"
import { OTLPMetricExporter } from "@opentelemetry/exporter-metrics-otlp-http/build/esm/platform/browser"
import { resourceFromAttributes } from "@opentelemetry/resources"
import { LoggerProvider } from "@opentelemetry/sdk-logs"
import { BatchLogRecordProcessor } from "@opentelemetry/sdk-logs/build/esm/platform/browser"
import { MeterProvider, PeriodicExportingMetricReader } from "@opentelemetry/sdk-metrics"

import { version } from "../../package.json"

const SERVICE_NAME = "ariadne-desktop"
const EXPORT_INTERVAL_MS = 5_000

let diagnosticsLoggerInstalled = false

type LogSeverity = "trace" | "debug" | "info" | "warn" | "error"
type LogContext = string | Record<string, string | number>

const SEVERITY: Record<LogSeverity, SeverityNumber> = {
  trace: SeverityNumber.TRACE,
  debug: SeverityNumber.DEBUG,
  info: SeverityNumber.INFO,
  warn: SeverityNumber.WARN,
  error: SeverityNumber.ERROR,
}

interface TelemetryOptions {
  baseUrl: string
  enabled: boolean
}

/** The desktop's OTLP logs and metrics, configured from its persisted settings. */
class Telemetry {
  #enabled: boolean
  #loggerProvider?: LoggerProvider
  #meterProvider?: MeterProvider
  #logger?: ReturnType<LoggerProvider["getLogger"]>
  #apiDuration?: ReturnType<ReturnType<MeterProvider["getMeter"]>["createHistogram"]>
  #streamReconnects?: ReturnType<ReturnType<MeterProvider["getMeter"]>["createCounter"]>
  #navigations?: ReturnType<ReturnType<MeterProvider["getMeter"]>["createCounter"]>

  constructor({ baseUrl, enabled }: TelemetryOptions) {
    this.#enabled = enabled
    if (!enabled) return
    installDiagnosticsLogger()

    const resource = resourceFromAttributes({
      "service.name": SERVICE_NAME,
      "service.version": version,
    })
    const logs = new OTLPLogExporter({ url: otlpUrl(baseUrl, "logs") })
    const metrics = new OTLPMetricExporter({ url: otlpUrl(baseUrl, "metrics") })
    this.#loggerProvider = new LoggerProvider({
      resource,
      processors: [
        new BatchLogRecordProcessor({ exporter: logs, scheduledDelayMillis: EXPORT_INTERVAL_MS }),
      ],
    })
    this.#meterProvider = new MeterProvider({
      resource,
      readers: [
        new PeriodicExportingMetricReader({
          exporter: metrics,
          exportIntervalMillis: EXPORT_INTERVAL_MS,
        }),
      ],
    })
    this.#logger = this.#loggerProvider.getLogger(SERVICE_NAME, version)
    const meter = this.#meterProvider.getMeter(SERVICE_NAME, version)
    this.#apiDuration = meter.createHistogram("ariadne.ui.api.request.duration", { unit: "ms" })
    this.#streamReconnects = meter.createCounter("ariadne.ui.stream.reconnects")
    this.#navigations = meter.createCounter("ariadne.ui.navigations")
  }

  log(severity: LogSeverity, message: string, contextValue?: LogContext): void {
    const attributes =
      typeof contextValue === "string" ? { "exception.stacktrace": contextValue } : contextValue
    const target = typeof contextValue === "object" ? contextValue.target : undefined
    const logger =
      target && this.#loggerProvider
        ? this.#loggerProvider.getLogger(String(target), version)
        : this.#logger
    logger?.emit({
      severityNumber: SEVERITY[severity],
      severityText: severity.toUpperCase(),
      body: message,
      attributes,
      context: context.active(),
    })
  }

  apiRequest(method: string, route: string, status: number, durationMs: number): void {
    this.#apiDuration?.record(durationMs, {
      operation: `${method} ${route}`,
      status: String(status),
    })
  }

  streamReconnect(reason: string): void {
    this.#streamReconnects?.add(1, { reason })
  }

  navigation(route: string): void {
    this.#navigations?.add(1, { route })
  }

  async flush(): Promise<void> {
    if (!this.#enabled) return
    try {
      await this.#loggerProvider?.forceFlush()
      await this.#meterProvider?.forceFlush()
    } catch (error) {
      console.debug("[telemetry] export failed", error)
    }
  }

  async shutdown(): Promise<void> {
    if (!this.#enabled) return
    await this.flush()
    await Promise.all([this.#loggerProvider?.shutdown(), this.#meterProvider?.shutdown()])
  }
}

let telemetry = new Telemetry({ baseUrl: "", enabled: false })

export function configureTelemetry(options: TelemetryOptions): void {
  const previous = telemetry
  telemetry = new Telemetry(options)
  void previous.shutdown()
}

export async function flushTelemetry(): Promise<void> {
  await telemetry.flush()
}

export function recordApiRequest(
  method: string,
  route: string,
  status: number,
  durationMs: number,
): void {
  telemetry.apiRequest(method, route, status, durationMs)
}

export function recordStreamReconnect(reason: string): void {
  telemetry.streamReconnect(reason)
}

export function recordNavigation(route: string): void {
  telemetry.navigation(route)
}

export function recordLog(severity: LogSeverity, message: string, contextValue?: LogContext): void {
  telemetry.log(severity, message, contextValue)
}

function otlpUrl(baseUrl: string, signal: "logs" | "metrics"): string {
  return new URL(`/otlp/v1/${signal}`, `${baseUrl.replace(/\/+$/, "")}/`).toString()
}

/** Exporters report asynchronous failures through OpenTelemetry's diagnostic logger. */
function installDiagnosticsLogger(): void {
  if (diagnosticsLoggerInstalled) return
  diagnosticsLoggerInstalled = true
  diag.setLogger(
    {
      error: () => console.debug("[telemetry] export failed"),
      warn: () => {},
      info: () => {},
      debug: () => {},
      verbose: () => {},
    },
    DiagLogLevel.ERROR,
  )
}
