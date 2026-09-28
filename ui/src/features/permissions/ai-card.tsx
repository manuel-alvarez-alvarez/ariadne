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
 * The schedule is the one control with an "off" state of its own: a time
 * input's native clear is what asks for it, so clearing it and picking a time
 * are the same gesture in both directions, and there is nothing to build for
 * it beyond reading an empty value as `null`.
 */

import { FlaskConicalIcon, RefreshCwIcon } from "lucide-react"
import { useEffect, useState } from "react"
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
import { Input } from "@/components/ui/input"
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

export function AiCard({ status }: { status: AiPermissionsStatusDto }) {
  const update = useUpdateAiPermissions()
  const refresh = useRefreshAiPermissions()

  // A typed buffer for the free-form fields, so a keystroke is not fought by
  // the row the last one answered with — reset only when the daemon's own
  // value actually moves (a write's own answer, another window, the stream).
  const [schedule, setSchedule] = useState(status.schedule ?? "")
  useEffect(() => setSchedule(status.schedule ?? ""), [status.schedule])

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
        <SectionHeading>Daily refresh</SectionHeading>
        <Field>
          <FieldLabel htmlFor="ai-schedule">Daily refresh</FieldLabel>
          <Input
            id="ai-schedule"
            type="time"
            className="w-32"
            value={schedule}
            aria-label="Daily refresh"
            onChange={(event) => setSchedule(event.target.value)}
            onBlur={() => {
              if (schedule === (status.schedule ?? "")) return
              send(
                { schedule: schedule === "" ? null : schedule },
                "Could not change the daily refresh",
              )
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur()
            }}
          />
          <FieldDescription>
            When the install runs again on its own, local time. Clear it to turn the refresh off.
          </FieldDescription>
        </Field>
      </div>

      <div className="flex flex-col gap-2">
        <SectionHeading>Status</SectionHeading>
        <FactList columns={3} framed={false}>
          <Fact label="Installed release">{status.installed_release ?? "—"}</Fact>
          <Fact label="Latest release">{status.latest_release ?? "—"}</Fact>
          <Fact label="Weights present">{status.weights_present ? "Yes" : "No"}</Fact>
          <Fact label="Endpoint">{status.endpoint ?? "—"}</Fact>
          <Fact label="Last refresh">
            <When at={status.last_refresh_at} format="age" />
          </Fact>
        </FactList>
      </div>
    </div>
  )
}
