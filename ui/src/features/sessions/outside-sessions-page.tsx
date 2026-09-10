/**
 * Sessions found in the transcript stores of the supported CLIs, but not
 * started by Ariadne. The user can make one the author of a ready task here,
 * which is the desktop equivalent of `ariadne session discover` and
 * `ariadne session adopt`.
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
import { parseModelRef } from "@/features/models/model-ref"
import { taskAuthor } from "@/features/tasks/agents"
import { taskListQueryOptions } from "@/features/tasks/queries"
import { AGENT_KIND_LABELS, cn } from "@/lib/format"

import { outsideSessionsQueryOptions, useAdoptOutsideSession } from "./queries"

export function OutsideSessionsPage() {
  const sessions = useQuery(outsideSessionsQueryOptions())
  const [selected, setSelected] = useState<OutsideSessionDto | null>(null)

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Outside sessions"
        description="Conversations started in a supported CLI that Ariadne can continue as a task author."
      />
      <DataTable
        query={sessions}
        errorTitle="Could not load outside sessions"
        columns={[
          { header: "CLI" },
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
        rowKey={(session) => `${session.agent_kind}:${session.internal_session_id}`}
        renderRow={(session) => (
          <TableRow>
            <TableCell>{AGENT_KIND_LABELS[session.agent_kind]}</TableCell>
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
                aria-label={`Adopt ${AGENT_KIND_LABELS[session.agent_kind]} session`}
                onClick={() => setSelected(session)}
              >
                Adopt
              </Button>
            </TableCell>
          </TableRow>
        )}
      />
      <AdoptOutsideSessionDialog session={selected} onClose={() => setSelected(null)} />
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
    () => (session ? (tasks.data ?? []).filter((task) => taskUsesCli(task, session)) : []),
    [session, tasks.data],
  )

  function close() {
    setTaskId(null)
    adopt.reset()
    onClose()
  }

  if (!session) return null
  const selectedTask = readyTasks.find((task) => task.id === taskId)

  return (
    <ConfirmDialog
      open
      onClose={close}
      title="Adopt this session?"
      description={`Ariadne will continue this ${AGENT_KIND_LABELS[session.agent_kind]} conversation as the task author.`}
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
        <p className="text-sm text-muted-foreground">
          No ready task uses {AGENT_KIND_LABELS[session.agent_kind]} as its author.
        </p>
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

function taskUsesCli(task: TaskDto, session: OutsideSessionDto): boolean {
  return parseModelRef(taskAuthor(task)?.model ?? "")?.agentKind === session.agent_kind
}
