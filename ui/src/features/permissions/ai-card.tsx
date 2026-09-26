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

import { useEffect, useState } from "react"
import { toast } from "sonner"

import type { AiPermissionsStatusDto, PythonDto, UpdateAiPermissionsRequest } from "@/api"
import { Fact, FactList } from "@/components/fact-list"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { When } from "@/components/when"
import { describeError } from "@/lib/format"

import { useRefreshAiPermissions, useUpdateAiPermissions } from "./queries"

/** What the Python check found, in the one line that says why the switch is off. */
function pythonReason(python: PythonDto): string {
  const found = python.version ? `found ${python.version}` : "not found"
  return `The model needs Python 3.10 or newer; ${found}.`
}

const STATE_LABELS: Record<AiPermissionsStatusDto["state"], string> = {
  disabled: "Disabled",
  installing: "Installing",
  ready: "Ready",
  failed: "Failed",
}

export function AiCard({ status }: { status: AiPermissionsStatusDto }) {
  const update = useUpdateAiPermissions()
  const refresh = useRefreshAiPermissions()

  // A typed buffer for the free-form fields, so a keystroke is not fought by
  // the row the last one answered with — reset only when the daemon's own
  // value actually moves (a write's own answer, another window, the stream).
  const [thresholdText, setThresholdText] = useState(String(status.threshold))
  useEffect(() => setThresholdText(String(status.threshold)), [status.threshold])
  const [schedule, setSchedule] = useState(status.schedule ?? "")
  useEffect(() => setSchedule(status.schedule ?? ""), [status.schedule])

  function send(body: UpdateAiPermissionsRequest, failureTitle: string) {
    update.mutate(body, {
      onError: (error) => toast.error(failureTitle, { description: describeError(error) }),
    })
  }

  return (
    <div className="flex flex-col gap-4 rounded-xl border bg-card p-4">
      <div>
        <h2 className="font-heading text-base font-semibold">AI</h2>
        <p className="text-sm text-muted-foreground">
          A model that runs on this machine and answers the `ai` permission mode's requests.
        </p>
      </div>

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

      <Field>
        <FieldLabel htmlFor="ai-threshold">Threshold</FieldLabel>
        <Input
          id="ai-threshold"
          type="number"
          min={0}
          max={1}
          step={0.05}
          value={thresholdText}
          aria-label="Threshold"
          onChange={(event) => setThresholdText(event.target.value)}
          onBlur={(event) => {
            const value = event.target.valueAsNumber
            if (!Number.isNaN(value)) send({ threshold: value }, "Could not change the threshold")
          }}
        />
        <FieldDescription>
          How sure the model has to be before its answer is taken, 0 to 1.
        </FieldDescription>
      </Field>

      <Field>
        <FieldLabel htmlFor="ai-schedule">Daily refresh</FieldLabel>
        <Input
          id="ai-schedule"
          type="time"
          value={schedule}
          aria-label="Daily refresh"
          onChange={(event) => {
            const value = event.target.value
            setSchedule(value)
            send({ schedule: value === "" ? null : value }, "Could not change the daily refresh")
          }}
        />
        <FieldDescription>
          When the install runs again on its own, local time. Clear it to turn the refresh off.
        </FieldDescription>
      </Field>

      <div>
        <Button
          variant="outline"
          disabled={
            status.state === "disabled" || status.state === "installing" || refresh.isPending
          }
          pending={refresh.isPending}
          onClick={() =>
            refresh.mutate(undefined, {
              onError: (error) =>
                toast.error("Could not refresh the model", { description: describeError(error) }),
            })
          }
        >
          Refresh
        </Button>
      </div>

      <FactList columns={3} framed={false}>
        <Fact label="State">{STATE_LABELS[status.state]}</Fact>
        <Fact label="Installed release">{status.installed_release ?? "—"}</Fact>
        <Fact label="Latest release">{status.latest_release ?? "—"}</Fact>
        <Fact label="Weights present">{status.weights_present ? "Yes" : "No"}</Fact>
        <Fact label="Endpoint">{status.endpoint ?? "—"}</Fact>
        <Fact label="Last refresh">
          <When at={status.last_refresh_at} format="age" />
        </Fact>
        {status.last_error ? (
          <Fact label="Last error" className="sm:col-span-2 lg:col-span-3">
            <span className="text-destructive">{status.last_error}</span>
          </Fact>
        ) : null}
      </FactList>
    </div>
  )
}
