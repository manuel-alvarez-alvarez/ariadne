/**
 * Saved memory: the facts sessions and the user have kept for later, with a
 * search box that queries the daemon rather than filtering in the browser, a
 * scope filter, add and delete — matching `ariadne memory
 * ls|search|add|delete` (014's parity rule, spec 015).
 *
 * Two pages share this screen. `#/memory` holds every scope: the global
 * memories and every repository's. A repository's page, reached from its row
 * on the repositories screen, holds that repository's memories and the global
 * ones, and its add form saves for that repository alone.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon, Trash2Icon } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"

import type { MemoryDto, RepositoryDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName, plural, shortId } from "@/lib/format"
import { paths } from "@/routes/paths"

import { AddMemoryDialog, GLOBAL_SCOPE } from "./add-memory-dialog"
import { DeleteMemoryDialog } from "./delete-memory-dialog"
import { type MemoryListFilters, memoriesQueryOptions } from "./queries"

const COLUMNS = [
  { header: "Text", className: "min-w-48" },
  { header: "Scope" },
  { header: "Source" },
  { header: "Expires" },
  { className: "w-10 text-right" },
]

/** The scope filter's value for every scope the page holds. */
const ALL_SCOPES = "all"
/** The repository page's filter value for its own memories alone. */
const OWN_SCOPE = "repository"

export function MemoryPage({ repositoryId }: { repositoryId?: string }) {
  const repositories = useQuery(repositoriesQueryOptions())
  const repository = repositories.data?.find((row) => row.id === repositoryId)

  const [query, setQuery] = useState("")
  const [scope, setScope] = useState(ALL_SCOPES)
  const memories = useQuery(memoriesQueryOptions(listFilters(scope, repositoryId), query))

  const [adding, setAdding] = useState(false)
  const [deleting, setDeleting] = useState<MemoryDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)

  function openDelete(memory: MemoryDto) {
    setDeleting(memory)
    setDeleteOpen(true)
  }

  if (repositoryId !== undefined) {
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
  }

  const known = repositories.data ?? []
  const scopes = repository
    ? [
        { label: "This repository and global", value: ALL_SCOPES },
        { label: folderName(repository.path), value: OWN_SCOPE },
        { label: "Global", value: GLOBAL_SCOPE },
      ]
    : [
        { label: "All scopes", value: ALL_SCOPES },
        { label: "Global", value: GLOBAL_SCOPE },
        ...known.map((row) => ({ label: folderName(row.path), value: row.id })),
      ]

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        description={
          repository
            ? `What ${folderName(repository.path)}'s sessions and you have saved for later, and the global memories.`
            : "What sessions and you have saved for later: the global memories and every repository's."
        }
        actions={
          <>
            {repository ? (
              <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
                Back to repositories
              </Button>
            ) : null}
            <Button size="sm" onClick={() => setAdding(true)}>
              <PlusIcon />
              Add memory
            </Button>
          </>
        }
      />

      <div className="flex gap-2">
        <Input
          value={query}
          placeholder="Search memory"
          aria-label="Search memory"
          autoComplete="off"
          onChange={(event) => setQuery(event.target.value)}
        />
        <Select
          value={scope}
          onValueChange={(value) => setScope(value ?? ALL_SCOPES)}
          items={scopes}
        >
          <SelectTrigger aria-label="Filter by scope" className="w-56 shrink-0">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {scopes.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {memories.data?.fallback ? (
        <p className="text-sm">
          No word of “{query.trim()}” matched. The newest memories stand in.
        </p>
      ) : null}

      {memories.data ? (
        <p className="text-sm text-muted-foreground">
          {plural(memories.data.hits.length, "memory", "memories")}
        </p>
      ) : null}

      <DataTable
        query={{ ...memories, data: memories.data?.hits }}
        errorTitle="Could not load memory"
        columns={COLUMNS}
        empty={<EmptyState className="border-0 py-12" title={emptyTitle(query, repository)} />}
        rowKey={(memory) => memory.id}
        renderRow={(memory) => (
          <MemoryRow memory={memory} repositories={known} onDelete={() => openDelete(memory)} />
        )}
      />

      <AddMemoryDialog
        open={adding}
        onOpenChange={setAdding}
        repositories={known}
        repositoryId={repositoryId}
      />
      <DeleteMemoryDialog open={deleteOpen} onOpenChange={setDeleteOpen} memory={deleting} />
    </div>
  )
}

/** What the scope filter's value asks the daemon for. */
function listFilters(scope: string, repositoryId: string | undefined): MemoryListFilters {
  if (scope === GLOBAL_SCOPE) return { scope: "global" }
  if (repositoryId !== undefined) {
    return scope === OWN_SCOPE
      ? { repository: repositoryId, scope: "repository" }
      : { repository: repositoryId }
  }
  return scope === ALL_SCOPES ? {} : { repository: scope, scope: "repository" }
}

/** Mirrors the CLI's own two empty states (`ariadne memory ls|search`). */
function emptyTitle(query: string, repository: RepositoryDto | undefined): string {
  const trimmed = query.trim()
  if (trimmed) return `No memory matches “${trimmed}”.`
  return repository ? "No active memories for this repository." : "No active memories."
}

function MemoryRow({
  memory,
  repositories,
  onDelete,
}: {
  memory: MemoryDto
  repositories: RepositoryDto[]
  onDelete: () => void
}) {
  const owner = repositories.find((row) => row.id === memory.repository_id)
  return (
    <TableRow>
      <TableCell className="min-w-48 whitespace-normal">{memory.text}</TableCell>
      <TableCell className="whitespace-nowrap">
        {!memory.repository_id
          ? "Global"
          : owner
            ? folderName(owner.path)
            : shortId(memory.repository_id)}
      </TableCell>
      <TableCell className="whitespace-nowrap text-muted-foreground text-xs">
        <MemorySource memory={memory} />
      </TableCell>
      <TableCell className="whitespace-nowrap">
        {memory.expires_at ? <When at={memory.expires_at} label="expires" /> : "never"}
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

/** The session, task and goal that saved it; a user write records none. */
function MemorySource({ memory }: { memory: MemoryDto }) {
  if (!memory.source_session_id) return "you"
  return (
    <>
      session <CopyableId value={memory.source_session_id} display={shortId} label="session id" />
      {" · task "}
      {memory.source_task_id ? (
        <CopyableId value={memory.source_task_id} display={shortId} label="task id" />
      ) : (
        "—"
      )}
      {" · goal "}
      {memory.source_goal_id ? (
        <CopyableId value={memory.source_goal_id} display={shortId} label="goal id" />
      ) : (
        "—"
      )}
    </>
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
