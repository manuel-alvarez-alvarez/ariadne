/**
 * The stored sessions of every ACP agent that can list them, none of them
 * started by Ariadne. The user can make one the author of a ready task here, which is
 * the desktop equivalent of `ariadne session discover` and `ariadne session
 * adopt`.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo, useState } from "react"
import { toast } from "sonner"

import type { OutsideSessionDto, TaskDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { DataTable } from "@/components/data-table"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { acpAgentsQueryOptions } from "@/features/agents/queries"
import { parseModelRef } from "@/features/models/model-ref"
import { taskAuthor } from "@/features/tasks/agents"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { cn } from "@/lib/format"

import { outsideSessionsQueryOptions, useAdoptOutsideSession } from "./queries"

/**
 * The agent cell and the dialog's wording name the agent by its registry id,
 * which is what tells one ACP agent from another.
 */
function sessionAgentLabel(session: OutsideSessionDto): string {
  return session.agent_id
}

export function OutsideSessionsPage() {
  const sessions = useQuery(outsideSessionsQueryOptions())
  const [selected, setSelected] = useState<OutsideSessionDto | null>(null)

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Outside sessions"
        description="Conversations stored by an ACP agent that Ariadne can continue as a task author."
      />
      <DataTable
        query={sessions}
        errorTitle="Could not load outside sessions"
        columns={[
          { header: "Agent" },
          { header: "Working directory", className: "min-w-56" },
          { header: "Last activity", className: "text-right" },
          { header: "First prompt", className: "min-w-72" },
          {},
        ]}
        empty={
          <p className="px-4 py-8 text-center text-sm text-muted-foreground">
            No outside sessions found.
          </p>
        }
        rowKey={(session) => `${session.agent_id}:${session.internal_session_id}`}
        renderRow={(session) => (
          <TableRow>
            <TableCell>{sessionAgentLabel(session)}</TableCell>
            <TableCell
              className="max-w-sm truncate font-mono text-xs"
              title={session.working_directory}
            >
              {session.working_directory}
            </TableCell>
            <TableCell className="text-right">
              <When at={session.last_activity_at} format="age" label="last activity" />
            </TableCell>
            <TableCell className="max-w-xl truncate" title={session.first_prompt}>
              {session.first_prompt}
            </TableCell>
            <TableCell className="text-right">
              <Button
                variant="outline"
                size="sm"
                aria-label={`Adopt ${sessionAgentLabel(session)} session`}
                onClick={() => setSelected(session)}
              >
                Adopt
              </Button>
            </TableCell>
          </TableRow>
        )}
      />
      <UnavailableAcpAgents />
      <AdoptOutsideSessionDialog session={selected} onClose={() => setSelected(null)} />
    </div>
  )
}

/**
 * Why an ACP agent's sessions are missing from the table above: rejected
 * outright, or ready but without the `session_list` capability. Quiet where
 * every registered agent can list its own — which is also the case for
 * nobody having configured one at all.
 */
function UnavailableAcpAgents() {
  const agents = useQuery(acpAgentsQueryOptions())
  const unavailable = (agents.data ?? []).filter((agent) => !agent.capabilities.session_list)
  if (unavailable.length === 0) return null

  return (
    <div className="rounded-md border border-dashed p-3 text-sm text-muted-foreground">
      <p className="font-medium text-foreground">Adoption unavailable for some ACP agents</p>
      <ul className="mt-1 list-inside list-disc">
        {unavailable.map((agent) => (
          <li key={agent.id}>
            <span className="font-mono text-xs">{agent.id}</span>:{" "}
            {agent.rejection_reason ?? "the agent does not support listing sessions"}
          </li>
        ))}
      </ul>
    </div>
  )
}

function AdoptOutsideSessionDialog({
  session,
  onClose,
}: {
  session: OutsideSessionDto | null
  onClose: () => void
}) {
  const tasks = useQuery({
    ...taskListQueryOptions({ status: "ready" }),
    enabled: session !== null,
  })
  const adopt = useAdoptOutsideSession()
  const [taskId, setTaskId] = useState<string | null>(null)
  const readyTasks = useMemo(
    () => (session ? (tasks.data ?? []).filter((task) => taskUsesAgent(task, session)) : []),
    [session, tasks.data],
  )

  function close() {
    setTaskId(null)
    adopt.reset()
    onClose()
  }

  if (!session) return null
  const selectedTask = readyTasks.find((task) => task.id === taskId)
  const label = sessionAgentLabel(session)

  return (
    <ConfirmDialog
      open
      onClose={close}
      title="Adopt this session?"
      description={`Ariadne will continue this ${label} conversation as the task author.`}
      confirmLabel="Adopt session"
      confirmDisabled={!selectedTask}
      pending={adopt.isPending}
      error={adopt.error}
      errorTitle="Could not adopt session"
      onConfirm={() => {
        if (!selectedTask) return
        adopt.mutate(
          { taskId: selectedTask.id, ...session },
          {
            onSuccess: () => {
              toast.success("Session adopted", { description: selectedTask.title })
              close()
            },
          },
        )
      }}
    >
      {tasks.isError ? (
        <ErrorState
          title="Could not load ready tasks"
          error={tasks.error}
          onRetry={() => void tasks.refetch()}
        />
      ) : tasks.isPending ? (
        <p className="text-sm text-muted-foreground">Loading ready tasks…</p>
      ) : readyTasks.length === 0 ? (
        <p className="text-sm text-muted-foreground">No ready task uses {label} as its author.</p>
      ) : (
        <div className="grid gap-2">
          <p className="text-sm font-medium">Choose a ready task</p>
          {readyTasks.map((task) => (
            <Button
              key={task.id}
              type="button"
              variant="outline"
              aria-pressed={task.id === taskId}
              className={cn("justify-start", task.id === taskId && "border-primary")}
              onClick={() => setTaskId(task.id)}
            >
              Use {task.title}
            </Button>
          ))}
        </div>
      )}
    </ConfirmDialog>
  )
}

/**
 * Whether a ready task's author runs the same agent this session belongs to:
 * the registry id before the first `:` of its pin.
 */
function taskUsesAgent(task: TaskDto, session: OutsideSessionDto): boolean {
  return parseModelRef(taskAuthor(task)?.model ?? "")?.agent === session.agent_id
}
