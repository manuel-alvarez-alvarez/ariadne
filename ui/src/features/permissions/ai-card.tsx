/**
 * The AI permission model's settings, the Python check, and where the install
 * has got to — one card, because there is one row behind it
 * (`GET /v1/permissions/ai`) and nothing here is a list.
 *
 * Every control sends its own change the moment it is made: there is no Save
 * button, and the row the daemon answers with is what the facts below are
 * drawn from — the same pattern the agents screen's flag editor and rank
 * picker use. A refusal is toasted with the daemon's own message and the
 * control is left as it was, since the cached row was never touched.
 *
 */

import { FlaskConicalIcon, RefreshCwIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import type {
  AiPermissionsStatusDto,
  PythonDto,
  TestAiPermissionResponse,
  UpdateAiPermissionsRequest,
} from "@/api"
import { Fact, FactList } from "@/components/fact-list"
import { StatusBadge } from "@/components/status-badge"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { When } from "@/components/when"
import { describeError } from "@/lib/format"

import { AiTestPanel } from "./ai-test-panel"
import { useRefreshAiPermissions, useUpdateAiPermissions } from "./queries"
import { ThresholdRange } from "./threshold-range"

/** What the Python check found, in the one line that says why the switch is off. */
function pythonReason(python: PythonDto): string {
  const found = python.version ? `found ${python.version}` : "not found"
  return `The model needs Python 3.12 or 3.13; ${found}.`
}

const STATE_LABELS: Record<AiPermissionsStatusDto["state"], string> = {
  disabled: "Disabled",
  installing: "Installing",
  ready: "Ready",
  failed: "Failed",
}

/** The status ramp step each install state takes, from the same tokens as the task board. */
const STATE_TONE: Record<AiPermissionsStatusDto["state"], string> = {
  disabled: "bg-muted text-muted-foreground",
  installing: "bg-status-warn-soft text-status-warn-fg",
  ready: "bg-status-done-soft text-status-done-fg",
  failed: "bg-status-danger-soft text-status-danger-fg",
}

function SectionHeading({ children }: { children: string }) {
  return (
    <h3 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
      {children}
    </h3>
  )
}

const MEMORY_FORMAT = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 })
const MEMORY_UNITS = ["GB", "TB", "PB"]

function formatMemory(bytes: number): string {
  let value = bytes / 1024 ** 3
  let unit = 0
  while (value >= 1024 && unit < MEMORY_UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${MEMORY_FORMAT.format(value)} ${MEMORY_UNITS[unit]}`
}

function flavourReason(
  devices: AiPermissionsStatusDto["flavours"][number]["devices"],
): string | null {
  for (const device of ["cpu", "cuda", "mlx"] as const) {
    const reason = devices.find((option) => option.device === device)?.reason
    if (reason) return reason
  }
  return null
}

export function AiCard({ status }: { status: AiPermissionsStatusDto }) {
  const update = useUpdateAiPermissions()
  const refresh = useRefreshAiPermissions()

  // Held here, not in the test panel, because the range control needs the
  // danger too: this is what draws its marker, so a dragged or typed
  // threshold recolours the same score rather than losing it.
  const [testResult, setTestResult] = useState<TestAiPermissionResponse | null>(null)
  const [testOpen, setTestOpen] = useState(false)

  function send(body: UpdateAiPermissionsRequest, failureTitle: string) {
    update.mutate(body, {
      onError: (error) => toast.error(failureTitle, { description: describeError(error) }),
    })
  }

  const selectedFlavour = status.flavours.find(({ flavour }) => flavour === status.flavour)
  const selectsDisabled = status.state === "installing" || update.isPending

  return (
    <div className="flex flex-col gap-4 rounded-xl border bg-card p-4">
      {status.last_error ? (
        <Alert variant="destructive">
          <AlertTitle>Last error</AlertTitle>
          <AlertDescription>{status.last_error}</AlertDescription>
        </Alert>
      ) : null}

      <header className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="font-heading text-base font-semibold">AI</h2>
            <StatusBadge label={STATE_LABELS[status.state]} tone={STATE_TONE[status.state]} />
          </div>
          <p className="text-sm text-muted-foreground">
            A model that runs on this machine and answers the ai permission mode's requests.
          </p>
        </div>

        <ButtonGroup>
          <Button variant="outline" onClick={() => setTestOpen(true)}>
            <FlaskConicalIcon data-icon="inline-start" />
            Test a request
          </Button>
          <Button
            variant="outline"
            disabled={
              status.state === "disabled" || status.state === "installing" || refresh.isPending
            }
            pending={refresh.isPending}
            onClick={() =>
              refresh.mutate(undefined, {
                onError: (error) =>
                  toast.error("Could not refresh the model", {
                    description: describeError(error),
                  }),
              })
            }
          >
            <RefreshCwIcon data-icon="inline-start" />
            Refresh
          </Button>
        </ButtonGroup>
        <AiTestPanel
          open={testOpen}
          onOpenChange={setTestOpen}
          status={status}
          result={testResult}
          onResult={setTestResult}
        />
      </header>

      <Field orientation="horizontal">
        <FieldLabel htmlFor="ai-enabled">Enable the AI permission model</FieldLabel>
        <Switch
          id="ai-enabled"
          checked={status.enabled}
          disabled={!status.python.ok || update.isPending}
          onCheckedChange={(checked) =>
            send(
              { enabled: checked },
              checked ? "Could not turn the model on" : "Could not turn the model off",
            )
          }
        />
      </Field>
      {!status.python.ok ? (
        <FieldDescription>{pythonReason(status.python)}</FieldDescription>
      ) : null}

      <div className="flex flex-col gap-2">
        <SectionHeading>Thresholds</SectionHeading>
        <ThresholdRange
          allowThreshold={status.allow_threshold}
          denyThreshold={status.deny_threshold}
          danger={testResult && !testResult.ai_error ? (testResult.danger ?? undefined) : undefined}
          mutate={update.mutate}
        />
      </div>

      <div className="flex flex-col gap-2">
        <SectionHeading>Model</SectionHeading>
        <Field>
          <FieldLabel htmlFor="ai-flavour">Flavour</FieldLabel>
          <Select
            value={status.flavour}
            disabled={selectsDisabled}
            onValueChange={(flavour) => send({ flavour }, "Could not change the model flavour")}
          >
            <SelectTrigger id="ai-flavour" aria-label="Flavour" className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {status.flavours.map((option) => {
                const runnable = option.devices.some((device) => device.can_run)
                const reason = flavourReason(option.devices)
                return (
                  <SelectItem key={option.flavour} value={option.flavour} disabled={!runnable}>
                    {option.flavour}
                    {!runnable && reason ? ` — ${reason}` : ""}
                  </SelectItem>
                )
              })}
            </SelectContent>
          </Select>
        </Field>
        <Field>
          <FieldLabel htmlFor="ai-device">Device</FieldLabel>
          <Select
            value={status.device}
            disabled={selectsDisabled}
            onValueChange={(device) =>
              send({ flavour: status.flavour, device }, "Could not change the model device")
            }
          >
            <SelectTrigger id="ai-device" aria-label="Device" className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {selectedFlavour?.devices.map((option) => (
                <SelectItem key={option.device} value={option.device} disabled={!option.can_run}>
                  {option.device}
                  {option.slow ? " — slow on CPU" : ""}
                  {!option.can_run && option.reason ? ` — ${option.reason}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>

      <div className="flex flex-col gap-2">
        <SectionHeading>Status and hardware</SectionHeading>
        <FactList columns={3} framed={false}>
          <Fact label="Installed release">{status.installed_release ?? "—"}</Fact>
          <Fact label="Latest release">{status.latest_release ?? "—"}</Fact>
          <Fact label="Weights present">{status.weights_present ? "Yes" : "No"}</Fact>
          <Fact label="Endpoint">{status.endpoint ?? "—"}</Fact>
          <Fact label="Last refresh">
            <When at={status.last_refresh_at} format="age" />
          </Fact>
          <Fact label="Memory">{formatMemory(status.hardware.memory_bytes)}</Fact>
          <Fact label="GPU">
            {status.hardware.gpu
              ? `${status.hardware.gpu.name} (${formatMemory(status.hardware.gpu.vram_bytes)})`
              : "No GPU"}
          </Fact>
        </FactList>
      </div>
    </div>
  )
}
