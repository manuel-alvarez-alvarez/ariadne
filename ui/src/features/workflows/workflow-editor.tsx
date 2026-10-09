import { RefreshCwIcon, Trash2Icon, Undo2Icon } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import type { WorkflowDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { cn, describeError } from "@/lib/format"

import { useDeleteWorkflow, useResetWorkflow, useUpdateWorkflow } from "./queries"
import { WorkflowPreview } from "./workflow-preview"

export function WorkflowEditor({
  workflow,
  onDeleted,
  onDirtyChange,
}: {
  workflow: WorkflowDto
  onDeleted: () => void
  onDirtyChange?: (dirty: boolean) => void
}) {
  const [document, setDocument] = useState(workflow.document)
  const [changedElsewhere, setChangedElsewhere] = useState(false)
  const [confirmReset, setConfirmReset] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const name = useRef(workflow.name)
  const synced = useRef(workflow.document)
  const saving = useRef<string | null>(null)
  const draft = useRef(document)
  draft.current = document
  const update = useUpdateWorkflow()
  const reset = useResetWorkflow()
  const remove = useDeleteWorkflow()

  useEffect(() => {
    if (workflow.name !== name.current) {
      name.current = workflow.name
      synced.current = workflow.document
      saving.current = null
      setDocument(workflow.document)
      setChangedElsewhere(false)
      return
    }
    if (workflow.document === synced.current) return
    if (saving.current === workflow.document) {
      synced.current = workflow.document
      saving.current = null
      return
    }
    if (draft.current === synced.current) {
      synced.current = workflow.document
      setDocument(workflow.document)
    } else setChangedElsewhere(true)
  }, [workflow.name, workflow.document])

  const dirty = document !== workflow.document
  useEffect(() => {
    onDirtyChange?.(dirty)
    return () => onDirtyChange?.(false)
  }, [dirty, onDirtyChange])
  function loadRow() {
    synced.current = workflow.document
    setDocument(workflow.document)
    setChangedElsewhere(false)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 xl:flex-row">
      <section aria-label="Workflow editor" className="flex min-h-0 min-w-0 flex-1 flex-col gap-4">
        <header className="flex flex-wrap items-baseline gap-2">
          <h2 className="font-medium text-lg">{workflow.name}</h2>
          <Badge variant={workflow.builtin ? "secondary" : "outline"}>
            {workflow.builtin ? "shipped" : "yours"}
          </Badge>
        </header>
        {changedElsewhere ? (
          <Alert>
            <RefreshCwIcon />
            <AlertTitle>This workflow changed elsewhere</AlertTitle>
            <AlertDescription>
              Your edits are kept here. Load the new version to see it.
            </AlertDescription>
            <AlertAction>
              <Button size="sm" variant="outline" onClick={loadRow}>
                Load new version
              </Button>
            </AlertAction>
          </Alert>
        ) : null}
        <Field className="flex min-h-0 flex-1 flex-col">
          <FieldLabel htmlFor="workflow-document">Document</FieldLabel>
          <Textarea
            id="workflow-document"
            value={document}
            spellCheck={false}
            onChange={(event) => setDocument(event.target.value)}
            className="min-h-48 flex-1 resize-none font-mono text-xs"
          />
        </Field>
        {update.error ? (
          <p role="alert" className="text-destructive text-sm">
            {describeError(update.error)}
          </p>
        ) : null}
        <footer className="flex flex-wrap items-center gap-2">
          <Button
            disabled={!dirty || update.isPending}
            onClick={() => {
              saving.current = document
              update.mutate(
                { name: workflow.name, body: { document } },
                {
                  onError: (error) => {
                    saving.current = null
                    toast.error("Could not save workflow", { description: describeError(error) })
                  },
                },
              )
            }}
          >
            {update.isPending ? "Saving…" : "Save"}
          </Button>
          <Button variant="ghost" disabled={!dirty} onClick={loadRow}>
            Discard
          </Button>
          <span className="flex-1" />
          {workflow.builtin ? (
            <Button
              variant="outline"
              disabled={reset.isPending}
              onClick={() => setConfirmReset(true)}
            >
              <Undo2Icon />
              Reset
            </Button>
          ) : (
            <Button
              variant="outline"
              className={cn("text-destructive")}
              onClick={() => setConfirmDelete(true)}
            >
              <Trash2Icon />
              Delete
            </Button>
          )}
        </footer>
      </section>
      <WorkflowPreview document={document} />
      <ConfirmDialog
        open={confirmReset}
        onClose={() => setConfirmReset(false)}
        title={`Reset ${workflow.name}?`}
        description="Discard this workflow document and restore the shipped version."
        confirmLabel="Reset"
        pending={reset.isPending}
        error={reset.error}
        onConfirm={() =>
          reset.mutate(workflow.name, {
            onSuccess: () => setConfirmReset(false),
            onError: (error) =>
              toast.error("Could not reset workflow", { description: describeError(error) }),
          })
        }
      />
      <ConfirmDialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={`Delete ${workflow.name}?`}
        description="Delete this workflow permanently."
        confirmLabel="Delete"
        destructive
        pending={remove.isPending}
        error={remove.error}
        onConfirm={() =>
          remove.mutate(workflow.name, {
            onSuccess: () => {
              onDirtyChange?.(false)
              onDeleted()
            },
            onError: (error) =>
              toast.error("Could not delete workflow", { description: describeError(error) }),
          })
        }
      />
    </div>
  )
}
