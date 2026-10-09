import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useState } from "react"
import { Link, useBlocker, useSearchParams } from "react-router-dom"

import type { WorkflowDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/format"
import { WORKFLOW_PARAM } from "@/routes/paths"

import { CreateWorkflowDialog } from "./create-workflow-dialog"
import { workflowsQueryOptions } from "./queries"
import { WorkflowEditor } from "./workflow-editor"

export function WorkflowsPage() {
  const [createOpen, setCreateOpen] = useState(false)
  const [search, setSearch] = useSearchParams()
  const selectedName = search.get(WORKFLOW_PARAM)
  const workflows = useQuery(workflowsQueryOptions())
  const selected = workflows.data?.find((workflow) => workflow.name === selectedName)
  const [dirty, setDirty] = useState(false)
  const blocker = useBlocker(dirty)
  function select(name: string) {
    const next = new URLSearchParams(search)
    next.set(WORKFLOW_PARAM, name)
    setSearch(next)
  }
  function clearSelection() {
    const next = new URLSearchParams(search)
    next.delete(WORKFLOW_PARAM)
    setSearch(next, { replace: true })
  }
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Workflows"
        description="The ordered columns that move a task from work through its required checks."
        actions={
          <Button onClick={() => setCreateOpen(true)}>
            <PlusIcon />
            New workflow
          </Button>
        }
      />
      {workflows.isPending ? (
        <LoadingWorkflows />
      ) : workflows.isError ? (
        <ErrorState
          title="Could not load workflows"
          error={workflows.error}
          onRetry={() => void workflows.refetch()}
          showIcon
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6">
          <WorkflowList
            workflows={workflows.data}
            selectedName={selectedName}
            search={search}
            className={cn(selectedName ? "hidden md:flex" : "flex")}
          />
          <section
            aria-label="Selected workflow"
            className={cn(
              "min-h-0 min-w-0 flex-1 flex-col",
              selectedName ? "flex" : "hidden md:flex",
            )}
          >
            {selected ? (
              <WorkflowEditor
                workflow={selected}
                onDeleted={clearSelection}
                onDirtyChange={setDirty}
              />
            ) : selectedName ? (
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="No workflow by that name."
                action={
                  <Button variant="outline" size="sm" onClick={clearSelection}>
                    Back to the list
                  </Button>
                }
              />
            ) : (
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="Select a workflow, or write one."
              />
            )}
          </section>
        </div>
      )}
      <CreateWorkflowDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(workflow) => select(workflow.name)}
      />
      <ConfirmDialog
        open={blocker.state === "blocked"}
        onClose={() => {
          if (blocker.state === "blocked") blocker.reset()
        }}
        title="Discard changes?"
        description="This workflow has unsaved changes. Leaving now drops them."
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => {
          if (blocker.state === "blocked") blocker.proceed()
        }}
      />
    </div>
  )
}

function WorkflowList({
  workflows,
  selectedName,
  search,
  className,
}: {
  workflows: WorkflowDto[]
  selectedName: string | null
  search: URLSearchParams
  className?: string
}) {
  const groups = [
    {
      key: "shipped",
      title: "Shipped with Ariadne",
      workflows: workflows.filter((workflow) => workflow.builtin),
    },
    { key: "yours", title: "Yours", workflows: workflows.filter((workflow) => !workflow.builtin) },
  ].filter((group) => group.workflows.length > 0)
  function href(name: string) {
    const next = new URLSearchParams(search)
    next.set(WORKFLOW_PARAM, name)
    return `?${next.toString()}`
  }
  return (
    <nav aria-label="Workflows" className={cn("w-full min-w-0 flex-col gap-3 md:w-72", className)}>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups.map((group) => (
          <section key={group.key} aria-labelledby={`workflows-${group.key}`} className="mb-4">
            <h3
              id={`workflows-${group.key}`}
              className="border-b px-1 pb-2 text-sm font-medium text-muted-foreground"
            >
              {group.title}
            </h3>
            <ul className="flex flex-col gap-0.5">
              {group.workflows.map((workflow) => (
                <li key={workflow.name}>
                  <Link
                    to={href(workflow.name)}
                    aria-current={workflow.name === selectedName ? "true" : undefined}
                    className={cn(
                      "flex flex-col gap-0.5 rounded-md px-2 py-1.5 text-sm hover:bg-accent",
                      workflow.name === selectedName && "bg-accent",
                    )}
                  >
                    <span className="font-medium">{workflow.name}</span>
                    <span className="truncate text-muted-foreground text-xs">
                      {workflow.steps.map((step) => step.title).join(" · ")}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </nav>
  )
}

function LoadingWorkflows() {
  return (
    <div className="flex min-h-0 flex-1 gap-6">
      <div className="flex w-full flex-col gap-2 md:w-72">
        {["a", "b", "c"].map((row) => (
          <Skeleton key={row} className="h-10 w-full" />
        ))}
      </div>
      <Skeleton className="hidden min-h-0 flex-1 md:block" />
    </div>
  )
}
