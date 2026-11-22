/**
 * "Add machine": one step-by-step dialog for each way to add a host.
 *
 * The first step asks for the method. "Connect to a daemon that already runs"
 * asks for the name, the address and the colour, and sends `POST /v1/hosts`.
 * "Install Ariadne on a machine through SSH" has four steps:
 *
 * 1. Connect asks for the name and the SSH address. Next sends
 *    `POST /v1/hosts/{name}/setup/inspect`.
 * 2. Select shows what inspection found, with a default for each choice.
 *    Next sends `POST /v1/hosts/{name}/setup/review`.
 * 3. Review shows the plan in three groups. Install sends
 *    `POST /v1/hosts/{name}/setup`.
 * 4. Install shows the live progress (`setup-progress.tsx`).
 *
 * Back keeps every input. Select always reviews again, so Install always
 * sends the request that the user saw reviewed. The setup runs in the daemon,
 * so the dialog can close during Install with nothing to discard.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { type FormEvent, type ReactNode, useId, useState } from "react"
import { toast } from "sonner"
import type { CreateHost } from "@/api"
import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import type { components } from "@/api/schema"
import { ErrorState } from "@/components/error-state"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { describeError } from "@/lib/format"
import type { HostColor } from "@/lib/host-colors"
import { fieldOf, HostColorField, type HostField } from "./host-fields"
import {
  connectBlocker,
  EMPTY_DRAFT,
  type Inspection,
  type MachineDraft,
  type SetupRequest,
  selectBlocker,
  setupRequest,
} from "./machine-setup"
import {
  Advanced,
  type InstallStep,
  ReviewStep,
  SelectStep,
  StepBar,
  TextField,
} from "./machine-setup-steps"
import { useCreateHost } from "./queries"
import { SetupProgress } from "./setup-progress"

type Method = "install" | "connect"
type Phase = "method" | "daemon" | "connect" | "select" | "review" | "install"
type SetupReview = components["schemas"]["SetupReviewDto"]

const STEP: Partial<Record<Phase, InstallStep>> = {
  connect: "Connect",
  select: "Select",
  review: "Review",
  install: "Install",
}

interface DaemonValues {
  name: string
  endpoint: string
  remote: string
  /** "" is the default colour. */
  color: HostColor | ""
}

const EMPTY_DAEMON: DaemonValues = { name: "", endpoint: "", remote: "", color: "" }

export function AddMachineDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  // Each open starts from a blank dialog, never from the last attempt.
  const [session, setSession] = useState(0)
  const [wasOpen, setWasOpen] = useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) setSession((n) => n + 1)
  }
  return <AddMachineFlow key={session} open={open} onOpenChange={onOpenChange} />
}

/** What the one form of the dialog shows and does at the current step. */
interface StepView {
  description: string
  body: ReactNode
  submitLabel: string
  submit: () => void
  pending?: boolean
  disabledReason?: string | null
  error?: { title: string; error: unknown; description?: ReactNode } | null
  back?: Phase
}

/**
 * Every step renders into one form dialog, so the dialog box stays in place
 * from the first step to the last and only its content changes.
 */
function AddMachineFlow({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const client = useQueryClient()
  const [phase, setPhase] = useState<Phase>("method")
  const [method, setMethod] = useState<Method>("install")
  const [touched, setTouched] = useState(false)
  const [daemon, setDaemon] = useState<DaemonValues>(EMPTY_DAEMON)
  const [refusal, setRefusal] = useState<{ field: HostField | null; message: string } | null>(null)
  const [draft, setDraft] = useState<MachineDraft>(EMPTY_DRAFT)
  const change = (patch: (draft: MachineDraft) => MachineDraft) => {
    setTouched(true)
    setDraft(patch)
  }
  const changeDaemon = (patch: Partial<DaemonValues>) => {
    setTouched(true)
    setRefusal(null)
    setDaemon((values) => ({ ...values, ...patch }))
  }

  const name = draft.name.trim()
  const repositories = useQuery({
    queryKey: qk.repositories.list(LOCAL_HOST),
    queryFn: () => unwrap(api(LOCAL_HOST).GET("/v1/repositories")),
    enabled: method === "install",
  })
  const repos = repositories.data ?? []

  // An inspection and a review answer for the inputs they were made from
  // alone; a changed input makes each one stale.
  const connection = JSON.stringify({ name, ssh: draft.ssh.trim() })
  const [inspection, setInspection] = useState<{ connection: string; data: Inspection } | null>(
    null,
  )
  const [review, setReview] = useState<{ fingerprint: string; data: SetupReview } | null>(null)
  const inspected = inspection?.connection === connection ? inspection.data : null
  const request = inspected ? setupRequest(draft, inspected, repos) : null
  const fingerprint = JSON.stringify({ name, request })
  const reviewed = review?.fingerprint === fingerprint ? review.data : null

  const createHost = useCreateHost()
  const inspect = useMutation({
    mutationFn: async () => ({
      connection,
      data: await unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/inspect", {
          params: { path: { name } },
          body: { ssh: draft.ssh.trim() },
        }),
      ),
    }),
    onSuccess: (result) => {
      setInspection(result)
      setPhase("select")
    },
  })
  const preview = useMutation({
    mutationFn: async (body: SetupRequest) => ({
      fingerprint,
      data: await unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/review", {
          params: { path: { name } },
          body,
        }),
      ),
    }),
    onSuccess: (result) => {
      setReview(result)
      setPhase("review")
    },
  })
  const start = useMutation({
    mutationFn: (body: SetupRequest) =>
      unwrap(api(LOCAL_HOST).POST("/v1/hosts/{name}/setup", { params: { path: { name } }, body })),
    onSuccess: (progress) => {
      client.setQueryData(qk.hosts.setup(name), progress)
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
      setPhase("install")
    },
  })

  async function addDaemon() {
    const body: CreateHost = { name: daemon.name.trim(), endpoint: daemon.endpoint.trim() }
    const remote = daemon.remote.trim()
    if (remote) body.remote = remote
    body.color = daemon.color || null
    try {
      const created = await createHost.mutateAsync(body)
      toast.success("Machine added", { description: created.name })
      onOpenChange(false)
    } catch (error) {
      setRefusal({ field: fieldOf(error), message: describeError(error) })
    }
  }

  const installing = "Install Ariadne on a machine through SSH."
  const view = ((): StepView => {
    switch (phase) {
      case "method":
        return {
          description: "Choose how this machine reaches the new machine.",
          body: <MethodChoice method={method} onMethod={setMethod} />,
          submitLabel: "Next",
          submit: () => setPhase(method === "install" ? "connect" : "daemon"),
        }
      case "daemon":
        return {
          description: "Connect to a daemon that already runs.",
          body: <DaemonFields values={daemon} refusal={refusal} change={changeDaemon} />,
          submitLabel: "Add machine",
          submit: () => void addDaemon(),
          pending: createHost.isPending,
          disabledReason: !daemon.name.trim()
            ? "Enter a name."
            : !daemon.endpoint.trim()
              ? "Enter the address."
              : null,
          error:
            refusal && !refusal.field
              ? { title: "Could not add the machine", error: null, description: refusal.message }
              : null,
          back: "method",
        }
      case "connect":
        return {
          description: installing,
          body: <ConnectFields draft={draft} change={change} />,
          submitLabel: "Next",
          submit: () => (inspected ? setPhase("select") : inspect.mutate()),
          pending: inspect.isPending,
          disabledReason: connectBlocker(draft),
          error: inspect.error
            ? { title: "Could not inspect the new machine", error: inspect.error }
            : null,
          back: "method",
        }
      case "select":
        return {
          description: installing,
          body: inspected ? (
            <SelectStep
              draft={draft}
              inspection={inspected}
              repositories={repos}
              repositoriesError={
                repositories.error ? (
                  <ErrorState title="Could not read the repositories" error={repositories.error} />
                ) : null
              }
              change={change}
            />
          ) : null,
          submitLabel: "Next",
          submit: () => request && preview.mutate(request),
          pending: preview.isPending,
          disabledReason: !inspected
            ? "Go back to Connect and inspect the new machine."
            : repositories.isPending
              ? "Reading the repositories."
              : selectBlocker(draft, repos),
          error: preview.error
            ? { title: "Could not review the setup", error: preview.error }
            : null,
          back: "connect",
        }
      case "review":
        return {
          description: installing,
          body:
            reviewed && inspected ? (
              <ReviewStep review={reviewed} draft={draft} inspection={inspected} />
            ) : null,
          submitLabel: "Install",
          submit: () => request && start.mutate(request),
          pending: start.isPending,
          disabledReason: reviewed ? null : "The selection changed. Go back and review it again.",
          error: start.error ? { title: "Could not start the setup", error: start.error } : null,
          back: "select",
        }
      case "install":
        return {
          description: installing,
          body: (
            <>
              <SetupProgress name={name} />
              <p className="text-sm text-muted-foreground">
                You can close this dialog. The setup continues on this machine.
              </p>
            </>
          ),
          submitLabel: "Close",
          submit: () => onOpenChange(false),
        }
    }
  })()
  const step = STEP[phase]
  const back = view.back

  return (
    <FormDialog
      open={open}
      onOpenChange={onOpenChange}
      dirty={touched && phase !== "install"}
      discardTitle="Discard this machine?"
      discardDescription="Closing now drops what you entered."
    >
      <FormDialogContent
        className={phase === "daemon" ? "sm:max-w-lg" : "sm:max-w-2xl"}
        title="Add machine"
        description={view.description}
        onSubmit={(event: FormEvent<HTMLFormElement>) => {
          event.preventDefault()
          view.submit()
        }}
        error={view.error}
        submitLabel={view.submitLabel}
        pending={view.pending ?? false}
        disabledReason={view.disabledReason}
        onBack={
          back
            ? () => {
                inspect.reset()
                preview.reset()
                start.reset()
                setPhase(back)
              }
            : undefined
        }
        cancelLabel={phase === "install" ? null : "Cancel"}
      >
        {step ? <StepBar current={step} /> : null}
        <FormDialogBody key={phase}>{view.body}</FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}

function MethodChoice({
  method,
  onMethod,
}: {
  method: Method
  onMethod: (method: Method) => void
}) {
  const id = useId()
  const options: { value: Method; title: string; when: string }[] = [
    {
      value: "install",
      title: "Install Ariadne on a machine through SSH",
      when: "Use this when Ariadne is not on the new machine yet.",
    },
    {
      value: "connect",
      title: "Connect to a daemon that already runs",
      when: "Use this when the new machine already runs ariadned.",
    },
  ]
  return (
    <RadioGroup
      aria-label="Method"
      value={method}
      onValueChange={(value) => onMethod(value as Method)}
    >
      {options.map((option) => (
        <label
          key={option.value}
          htmlFor={`${id}-${option.value}`}
          className="flex cursor-pointer items-start gap-3 rounded-lg border p-3 has-data-checked:border-primary/40 has-data-checked:bg-primary/5"
        >
          <RadioGroupItem
            value={option.value}
            id={`${id}-${option.value}`}
            aria-labelledby={`${id}-${option.value}-title`}
            aria-describedby={`${id}-${option.value}-when`}
            className="mt-0.5"
          />
          <span className="flex flex-col gap-1">
            <span id={`${id}-${option.value}-title`} className="font-medium">
              {option.title}
            </span>
            <span id={`${id}-${option.value}-when`} className="text-muted-foreground">
              {option.when}
            </span>
          </span>
        </label>
      ))}
    </RadioGroup>
  )
}

function DaemonFields({
  values,
  refusal,
  change,
}: {
  values: DaemonValues
  refusal: { field: HostField | null; message: string } | null
  change: (patch: Partial<DaemonValues>) => void
}) {
  const at = (field: HostField) =>
    refusal?.field === field ? { message: refusal.message } : undefined
  return (
    <>
      <TextField
        label="Name"
        help="Letters, digits, - and _. The app shows the machine by this name."
        value={values.name}
        onChange={(name) => change({ name })}
        error={at("name")}
        autoFocus
      />
      <TextField
        label="Address"
        help="How this machine reaches the daemon: an SSH address, a URL or a socket path."
        placeholder="ssh://me@box, http://box:7676 or /path/to.sock"
        value={values.endpoint}
        onChange={(endpoint) => change({ endpoint })}
        error={at("endpoint")}
        mono
      />
      <HostColorField
        value={values.color}
        onChange={(color) => change({ color })}
        error={at("color")}
        description="Default draws the machine in the colour its name gets."
      />
      <Advanced open={!!at("remote")}>
        <TextField
          label="Remote socket"
          help="For an ssh:// address: the socket of the daemon on the new machine. Leave empty for the default."
          placeholder="~/.ariadne/ariadne.sock"
          value={values.remote}
          onChange={(remote) => change({ remote })}
          error={at("remote")}
          mono
        />
      </Advanced>
    </>
  )
}

function ConnectFields({
  draft,
  change,
}: {
  draft: MachineDraft
  change: (patch: (draft: MachineDraft) => MachineDraft) => void
}) {
  return (
    <>
      <TextField
        label="Name"
        help="Letters, digits, - and _. The new machine appears in the hosts list by this name."
        value={draft.name}
        onChange={(value) => change((d) => ({ ...d, name: value }))}
        autoFocus
      />
      <TextField
        label="SSH address"
        help="Ariadne uses the SSH keys of this machine. It never asks for a password."
        placeholder="ssh://me@box"
        value={draft.ssh}
        onChange={(value) => change((d) => ({ ...d, ssh: value }))}
        mono
      />
    </>
  )
}
