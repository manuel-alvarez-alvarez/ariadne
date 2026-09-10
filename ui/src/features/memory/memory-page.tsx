/**
 * One repository's memory: what an agent session has saved about it, the
 * search box that queries the daemon rather than filtering in the browser, and
 * delete.
 *
 * There is no way to add a memory here — an agent session is what saves one
 * (019) — so this screen is read, search and delete, matching `ariadne memory
 * ls|search|delete` (014's parity rule, spec 015).
 *
 * Reached from a row on the repositories screen rather than the sidebar: a
 * memory belongs to one repository, and there is no reason to pick one out of
 * a list that already has its own row for it.
 */

import { useQuery } from "@tanstack/react-query"
import { Trash2Icon } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"

import type { MemoryDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName, plural, shortId } from "@/lib/format"
import { paths } from "@/routes/paths"

import { DeleteMemoryDialog } from "./delete-memory-dialog"
import { memoriesQueryOptions } from "./queries"

const COLUMNS = [
  { header: "Text", className: "min-w-48" },
  { header: "Source" },
  { header: "Expires" },
  { className: "w-10 text-right" },
]

export function MemoryPage({ repositoryId }: { repositoryId: string }) {
  const repositories = useQuery(repositoriesQueryOptions())
  const repository = repositories.data?.find((row) => row.id === repositoryId)

  const [query, setQuery] = useState("")
  const memories = useQuery(memoriesQueryOptions(repositoryId, query))

  const [deleting, setDeleting] = useState<MemoryDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)

  function openDelete(memory: MemoryDto) {
    setDeleting(memory)
    setDeleteOpen(true)
  }

  if (repositories.isPending) return <LoadingMemory />

  if (repositories.isError) {
    return (
      <ErrorState
        title="Could not load the repository"
        error={repositories.error}
        onRetry={() => void repositories.refetch()}
        showIcon
      />
    )
  }

  if (!repository) {
    return (
      <EmptyState
        emphasis="quiet"
        className="my-12"
        title="No repository by that id."
        description="It may have been removed since the link was made."
        action={
          <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
            Back to repositories
          </Button>
        }
      />
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        description={`What ${folderName(repository.path)}'s sessions have saved for later — text, its source and when it expires.`}
        actions={
          <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
            Back to repositories
          </Button>
        }
      />

      <Input
        value={query}
        placeholder="Search memory"
        aria-label="Search memory"
        autoComplete="off"
        onChange={(event) => setQuery(event.target.value)}
      />

      {memories.data ? (
        <p className="text-sm text-muted-foreground">
          {plural(memories.data.length, "memory", "memories")}
        </p>
      ) : null}

      <DataTable
        query={memories}
        errorTitle="Could not load memory"
        columns={COLUMNS}
        empty={<EmptyState className="border-0 py-12" title={emptyTitle(query)} />}
        rowKey={(memory) => memory.id}
        renderRow={(memory) => <MemoryRow memory={memory} onDelete={() => openDelete(memory)} />}
      />

      <DeleteMemoryDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        repositoryId={repositoryId}
        memory={deleting}
      />
    </div>
  )
}

/** Mirrors the CLI's own two empty states (`ariadne memory ls|search`). */
function emptyTitle(query: string): string {
  const trimmed = query.trim()
  return trimmed ? `No memory matches “${trimmed}”.` : "No active memories for this repository."
}

function MemoryRow({ memory, onDelete }: { memory: MemoryDto; onDelete: () => void }) {
  return (
    <TableRow>
      <TableCell className="min-w-48 whitespace-normal">{memory.text}</TableCell>
      <TableCell className="whitespace-nowrap text-muted-foreground text-xs">
        session <CopyableId value={memory.source_session_id} display={shortId} label="session id" />
        {" · task "}
        {memory.source_task_id ? (
          <CopyableId value={memory.source_task_id} display={shortId} label="task id" />
        ) : (
          "—"
        )}
        {" · goal "}
        <CopyableId value={memory.source_goal_id} display={shortId} label="goal id" />
      </TableCell>
      <TableCell className="whitespace-nowrap">
        <When at={memory.expires_at} label="expires" />
      </TableCell>
      <TableCell className="text-right">
        <RowAction
          icon={<Trash2Icon />}
          label={`Delete memory ${shortId(memory.id)}`}
          onClick={onDelete}
        />
      </TableCell>
    </TableRow>
  )
}

function LoadingMemory() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-8 w-48" />
      <Skeleton className="h-9 w-full" />
      {[0, 1, 2].map((row) => (
        <Skeleton key={row} className="h-10 w-full" />
      ))}
    </div>
  )
}
