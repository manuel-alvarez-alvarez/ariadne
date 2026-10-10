import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useState } from "react"
import { useBlocker, useSearchParams } from "react-router-dom"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { EmptyState } from "@/components/empty-state"
import { EntityCombobox } from "@/components/entity-combobox"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
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
        <div className="flex min-h-0 flex-1 flex-col gap-4">
          <EntityCombobox
            id="workflow-combobox"
            label="Workflow"
            entityLabel="workflow"
            placeholder="Select a workflow…"
            selectedName={selectedName}
            onSelect={select}
            entities={workflows.data.map((workflow) => ({
              name: workflow.name,
              details: workflow.steps.map((step) => step.title).join(" · "),
              builtin: workflow.builtin,
            }))}
          />
          <section aria-label="Selected workflow" className="flex min-h-0 min-w-0 flex-1 flex-col">
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
                    Clear selection
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

function LoadingWorkflows() {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <Skeleton className="h-8 w-full sm:w-96" />
      <Skeleton className="min-h-0 flex-1" />
    </div>
  )
}
