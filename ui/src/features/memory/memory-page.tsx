/**
 * Saved memory: the facts sessions and the user have kept for later, with a
 * search box that queries the daemon rather than filtering in the browser, a
 * scope filter, add and delete — matching `ariadne memory
 * ls|search|add|delete` (014's parity rule, spec 015).
 *
 * `#/memory` holds every scope: the global memories and every repository's.
 * The scope filter is `?repository=<id>` (or `global`), so a link or a reload
 * opens the page narrowed to one repository.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon, Trash2Icon } from "lucide-react"
import { useState } from "react"
import { useSearchParams } from "react-router-dom"

import type { MemoryDto, RepositoryDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
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
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName, plural, shortId } from "@/lib/format"

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
/** The search param the scope filter rides in: a repository's id, or `global`. */
const REPOSITORY_PARAM = "repository"

export function MemoryPage() {
  const repositories = useQuery(repositoriesQueryOptions())

  const [query, setQuery] = useState("")
  const [search, setSearch] = useSearchParams()
  const scope = search.get(REPOSITORY_PARAM) ?? ALL_SCOPES
  const memories = useQuery(memoriesQueryOptions(listFilters(scope), query))

  const [adding, setAdding] = useState(false)
  const [deleting, setDeleting] = useState<MemoryDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)

  function openDelete(memory: MemoryDto) {
    setDeleting(memory)
    setDeleteOpen(true)
  }

  // In the URL rather than in state, so a reload keeps the filter.
  function changeScope(value: string) {
    const next = new URLSearchParams(search)
    if (value === ALL_SCOPES) next.delete(REPOSITORY_PARAM)
    else next.set(REPOSITORY_PARAM, value)
    setSearch(next, { replace: true })
  }

  const known = repositories.data ?? []
  const scopes = [
    { label: "All scopes", value: ALL_SCOPES },
    { label: "Global", value: GLOBAL_SCOPE },
    ...known.map((row) => ({ label: folderName(row.path), value: row.id })),
  ]

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        description="What sessions and you have saved for later: the global memories and every repository's."
        actions={
          <Button size="sm" onClick={() => setAdding(true)}>
            <PlusIcon />
            Add memory
          </Button>
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
          onValueChange={(value) => changeScope(value ?? ALL_SCOPES)}
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
        empty={<EmptyState className="border-0 py-12" title={emptyTitle(query)} />}
        rowKey={(memory) => memory.id}
        renderRow={(memory) => (
          <MemoryRow memory={memory} repositories={known} onDelete={() => openDelete(memory)} />
        )}
      />

      <AddMemoryDialog open={adding} onOpenChange={setAdding} repositories={known} />
      <DeleteMemoryDialog open={deleteOpen} onOpenChange={setDeleteOpen} memory={deleting} />
    </div>
  )
}

/** What the scope filter's value asks the daemon for. */
function listFilters(scope: string): MemoryListFilters {
  if (scope === GLOBAL_SCOPE) return { scope: "global" }
  return scope === ALL_SCOPES ? {} : { repository: scope, scope: "repository" }
}

/** Mirrors the CLI's own two empty states (`ariadne memory ls|search`). */
function emptyTitle(query: string): string {
  const trimmed = query.trim()
  return trimmed ? `No memory matches “${trimmed}”.` : "No active memories."
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
