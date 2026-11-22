/**
 * "Update" of one machine, from its row of the Hosts table: the same form
 * dialog as "Add machine", with no name and no SSH input — the saved setup
 * holds them.
 *
 * Select has two independent choices. "Ariadne programs" updates them to the
 * version of this machine, and its Advanced section names other programs: two
 * program paths or a release. "Agents and tools" lists each agent by name and
 * GitHub CLI; the saved selection of the setup starts selected. Update sends
 * `POST /v1/hosts/{name}/update`, which waits for running tasks, and the
 * dialog then shows the live progress (`setup-progress.tsx`). A failed update
 * goes back to Select with the selections kept, and Update tries again.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { type FormEvent, useState } from "react"
import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import type { components } from "@/api/schema"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { acpAgentsQueryOptions } from "../agents/queries"
import { agentName, EMPTY_DRAFT, type MachineDraft, sourceOf } from "./machine-setup"
import { Advanced, CheckLine, Section, TextField } from "./machine-setup-steps"
import type { HostSetup } from "./queries"
import { SetupProgress, useProgress } from "./setup-progress"

type UpdateRequest = components["schemas"]["UpdateRequest"]

interface UpdateDraft {
  programs: boolean
  program: MachineDraft
  agents: Record<string, boolean>
  github: boolean
}

export function UpdateMachineDialog({
  setup,
  onOpenChange,
}: {
  /** The saved setup of the machine; null while the dialog is closed. */
  setup: HostSetup | null
  onOpenChange: (open: boolean) => void
}) {
  // Each open starts from the saved setup, never from the last attempt.
  const [session, setSession] = useState(0)
  const [last, setLast] = useState(setup?.name ?? null)
  if ((setup?.name ?? null) !== last) {
    setLast(setup?.name ?? null)
    if (setup) setSession((n) => n + 1)
  }
  return setup ? <UpdateFlow key={session} setup={setup} onOpenChange={onOpenChange} /> : null
}

function UpdateFlow({
  setup,
  onOpenChange,
}: {
  setup: HostSetup
  onOpenChange: (open: boolean) => void
}) {
  const client = useQueryClient()
  const name = setup.name
  const [phase, setPhase] = useState<"select" | "progress">(
    setup.operation === "update" && setup.state !== "complete" ? "progress" : "select",
  )
  const [draft, setDraft] = useState<UpdateDraft>(() => ({
    programs: true,
    program: EMPTY_DRAFT,
    agents: Object.fromEntries((setup.tools?.agents ?? []).map((id) => [id, true])),
    github: setup.tools?.github ?? false,
  }))
  const registry = useQuery(acpAgentsQueryOptions(LOCAL_HOST))
  const ids = [
    ...new Set([...(registry.data ?? []).map((agent) => agent.id), ...Object.keys(draft.agents)]),
  ]

  const selectedAgents = ids.filter((id) => draft.agents[id])
  const toolsSelected = selectedAgents.length > 0 || draft.github
  const source = sourceOf(draft.program)
  const blocker = draft.programs ? (source.blocker ?? null) : null
  const disabledReason =
    blocker ?? (!draft.programs && !toolsSelected ? "Select the programs or a tool." : null)

  const start = useMutation({
    mutationFn: () => {
      const body: UpdateRequest = {
        ...(draft.programs ? { source: source.source ?? { kind: "daemon" } } : {}),
        ...(toolsSelected ? { tools: { agents: selectedAgents, github: draft.github } } : {}),
      }
      return unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/update", { params: { path: { name } }, body }),
      )
    },
    onSuccess: (progress) => {
      client.setQueryData(qk.hosts.update(name), progress)
      void client.invalidateQueries({ queryKey: qk.hosts.setups() })
      setPhase("progress")
    },
  })
  const progress = useProgress(name, "update")
  const state = progress.data?.state
  const stopped = phase === "progress" && (state === "incomplete" || state === "failed")

  const change = (patch: Partial<UpdateDraft>) => setDraft((d) => ({ ...d, ...patch }))
  const changeProgram = (patch: Partial<MachineDraft>) =>
    setDraft((d) => ({ ...d, program: { ...d.program, ...patch } }))

  return (
    <FormDialog open onOpenChange={onOpenChange} dirty={false}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title={`Update ${name}`}
        description={
          phase === "select"
            ? "The update waits for running tasks on the machine, then updates what you select."
            : "The update waits for running tasks on the machine. You can close this dialog. The update continues on this machine."
        }
        onSubmit={(event: FormEvent<HTMLFormElement>) => {
          event.preventDefault()
          if (phase === "select") start.mutate()
          else onOpenChange(false)
        }}
        error={
          phase === "select" && start.error
            ? { title: "Could not start the update", error: start.error }
            : null
        }
        submitLabel={phase === "select" ? "Update" : "Close"}
        pending={start.isPending}
        disabledReason={phase === "select" ? disabledReason : null}
        onBack={
          stopped
            ? () => {
                start.reset()
                setPhase("select")
              }
            : undefined
        }
        cancelLabel={phase === "select" ? "Cancel" : null}
      >
        <FormDialogBody>
          {phase === "select" ? (
            <>
              <Section title="Ariadne programs">
                <CheckLine
                  label="Update the Ariadne programs"
                  checked={draft.programs}
                  onChange={(programs) => change({ programs })}
                >
                  Updates them to the version of this machine. Change that under Advanced.
                </CheckLine>
              </Section>
              <Section
                title="Agents and tools"
                description="The update installs or updates each one you select. Leave all empty to keep them."
              >
                {ids.map((id) => (
                  <CheckLine
                    key={id}
                    label={agentName(id)}
                    checked={draft.agents[id] ?? false}
                    onChange={(checked) => change({ agents: { ...draft.agents, [id]: checked } })}
                  />
                ))}
                <CheckLine
                  label="GitHub CLI"
                  checked={draft.github}
                  onChange={(github) => change({ github })}
                />
              </Section>
              <Advanced>
                <TextField
                  label="CLI program path"
                  help="An ariadne program on this machine to copy. Fill both paths, or neither."
                  placeholder="/usr/local/bin/ariadne"
                  mono
                  value={draft.program.cli}
                  onChange={(cli) => changeProgram({ cli })}
                />
                <TextField
                  label="Daemon program path"
                  help="An ariadned program on this machine to copy."
                  placeholder="/usr/local/bin/ariadned"
                  mono
                  value={draft.program.daemon}
                  onChange={(daemon) => changeProgram({ daemon })}
                />
                <TextField
                  label="Release repository"
                  help="The GitHub repository of a release to install. Fill it with the tag, or neither."
                  placeholder="owner/repository"
                  mono
                  value={draft.program.releaseRepository}
                  onChange={(releaseRepository) => changeProgram({ releaseRepository })}
                />
                <TextField
                  label="Release tag"
                  help="The tag of that release."
                  placeholder="v1.0.0"
                  mono
                  value={draft.program.releaseTag}
                  onChange={(releaseTag) => changeProgram({ releaseTag })}
                />
              </Advanced>
            </>
          ) : (
            <SetupProgress name={name} operation="update" retry={() => start.mutateAsync()} />
          )}
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
