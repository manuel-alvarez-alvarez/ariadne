/**
 * The Install step of "Add machine" and the progress of an update: the live
 * progress of one machine operation, read from the settings daemon's
 * `GET /v1/hosts/{name}/setup` (or `/update`) every second while it is
 * `pending` or `running`.
 *
 * Each step shows its title. A step that needs work by hand shows why, the
 * command to run on the new machine with a copy control, and "Check again",
 * which sends `POST /v1/hosts/{name}/setup/resume`; an update passes its own
 * `retry` instead. The operation runs in the daemon, so the dialog around this can close at any time.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  CircleCheckIcon,
  CircleDashedIcon,
  CircleXIcon,
  HandIcon,
  LoaderCircleIcon,
} from "lucide-react"
import { useEffect, useId } from "react"

import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import type { components } from "@/api/schema"
import { CopyableId } from "@/components/copyable-id"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"

type SetupOperation = "setup" | "update"
type SetupDto = components["schemas"]["SetupDto"]
type SetupState = components["schemas"]["SetupState"]
type SetupStep = components["schemas"]["SetupStep"]
type ManualAction = components["schemas"]["ManualAction"]

const ACTIVE: SetupState[] = ["pending", "running"]

const SUMMARY: Record<SetupOperation, Record<SetupState, string>> = {
  setup: {
    pending: "Setup is about to start.",
    running: "Setup is running.",
    incomplete: "Setup stopped. Do the work below, then check again.",
    failed: "Setup failed. Do the work below, then check again.",
    complete: "Setup is complete. The new machine is in the hosts list.",
  },
  update: {
    pending: "The update is about to start.",
    running: "The update is running.",
    incomplete: "The update stopped. Do the work below, then check again.",
    failed: "The update failed. Do the work below, then check again.",
    complete: "The update is complete.",
  },
}

/** The progress of one operation of a machine, read every second while it is active. */
export function useProgress(name: string, operation: SetupOperation) {
  return useQuery({
    queryKey: operation === "update" ? qk.hosts.update(name) : qk.hosts.setup(name),
    queryFn: () =>
      operation === "update"
        ? unwrap(api(LOCAL_HOST).GET("/v1/hosts/{name}/update", { params: { path: { name } } }))
        : unwrap(api(LOCAL_HOST).GET("/v1/hosts/{name}/setup", { params: { path: { name } } })),
    refetchInterval: (query) =>
      query.state.data && ACTIVE.includes(query.state.data.state) ? 1000 : false,
  })
}

export function SetupProgress({
  name,
  operation = "setup",
  retry,
}: {
  name: string
  operation?: SetupOperation
  /** Starts the operation again; without it, "Check again" resumes the setup. */
  retry?: () => Promise<SetupDto>
}) {
  const client = useQueryClient()
  const status = useProgress(name, operation)
  const state = status.data?.state
  useEffect(() => {
    if (state === "complete") {
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
      void client.invalidateQueries({ queryKey: qk.hosts.setups() })
    }
  }, [client, state])
  const resume = useMutation({
    mutationFn: () =>
      retry
        ? retry()
        : unwrap(
            api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/resume", { params: { path: { name } } }),
          ),
    onSuccess: (progress) => {
      client.setQueryData(
        operation === "update" ? qk.hosts.update(name) : qk.hosts.setup(name),
        progress,
      )
      void client.invalidateQueries({ queryKey: qk.hosts.setups() })
    },
  })

  if (status.error && !status.data)
    return (
      <ErrorState
        title={operation === "update" ? "Could not read the update" : "Could not read the setup"}
        error={status.error}
      />
    )
  if (!status.data) return <p className="text-sm text-muted-foreground">Reading the setup…</p>

  const stopped = state === "incomplete" || state === "failed"
  const steps = status.data.steps
  const running = ACTIVE.includes(status.data.state)
    ? steps.findIndex((step) => !step.complete && !step.error)
    : -1
  const manual = steps.some((step) => !step.complete && step.manual)
  return (
    <div className="flex flex-col gap-3">
      <p role="status" className="text-sm font-medium">
        {SUMMARY[operation][status.data.state]}
      </p>
      <ol aria-label="Setup steps" className="flex flex-col gap-2">
        {steps.map((step, index) => (
          <StepRow
            key={step.name}
            step={step}
            running={index === running}
            onCheckAgain={stopped ? () => resume.mutate() : null}
            checking={resume.isPending}
          />
        ))}
      </ol>
      {stopped && !manual ? (
        <Button
          type="button"
          variant="outline"
          className="self-start"
          pending={resume.isPending}
          onClick={() => resume.mutate()}
        >
          Try again
        </Button>
      ) : null}
      {resume.error ? (
        <ErrorState
          title={
            operation === "update" ? "Could not retry the update" : "Could not resume the setup"
          }
          error={resume.error}
        />
      ) : null}
    </div>
  )
}

function StepRow({
  step,
  running,
  onCheckAgain,
  checking,
}: {
  step: SetupStep
  running: boolean
  /** Resumes the setup; null while the setup still runs. */
  onCheckAgain: (() => void) | null
  checking: boolean
}) {
  const titleId = useId()
  const manual = step.complete ? null : step.manual
  const state = step.complete
    ? "Done"
    : step.error
      ? "Failed"
      : manual
        ? "Needs you"
        : running
          ? "Running"
          : "Waiting"
  const Icon = step.complete
    ? CircleCheckIcon
    : step.error
      ? CircleXIcon
      : manual
        ? HandIcon
        : running
          ? LoaderCircleIcon
          : CircleDashedIcon
  return (
    <li className="flex gap-2">
      <Icon
        aria-hidden
        className={cn(
          "mt-0.5 size-4 shrink-0",
          step.complete && "text-emerald-600 dark:text-emerald-400",
          step.error && "text-destructive",
          !step.complete && !step.error && "text-muted-foreground",
          running && "animate-spin",
        )}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p>
          <span id={titleId}>{step.title}</span>
          <span className="sr-only">: {state}</span>
        </p>
        {step.error ? <p className="text-sm text-destructive">{step.error}</p> : null}
        {manual ? (
          <ManualBlock
            manual={manual}
            titleId={titleId}
            onCheckAgain={onCheckAgain}
            checking={checking}
          />
        ) : null}
      </div>
    </li>
  )
}

function ManualBlock({
  manual,
  titleId,
  onCheckAgain,
  checking,
}: {
  manual: ManualAction
  titleId: string
  onCheckAgain: (() => void) | null
  checking: boolean
}) {
  return (
    <div className="flex flex-col gap-2 rounded-md border bg-muted/40 p-2 text-sm">
      <p>{manual.reason}</p>
      {manual.command ? <CommandLine command={manual.command} /> : null}
      {onCheckAgain ? (
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="self-start"
          aria-describedby={titleId}
          pending={checking}
          onClick={onCheckAgain}
        >
          Check again
        </Button>
      ) : null}
    </div>
  )
}

/** A command to run on the machine, with a button that copies it. */
export function CommandLine({ command }: { command: string }) {
  return (
    <CopyableId
      value={command}
      label="command"
      wrap
      className="rounded-sm bg-background px-2 py-1 text-xs"
    />
  )
}
