/**
 * The Learned tab: every approval a repository's agents no longer have to ask
 * for again, whichever permission mode left it — a console pick under `learn`
 * or `ai`, or one added by hand ahead of either.
 *
 * A repository is shown by its path, the one thing about it a person reads
 * (`RepositoryDto` carries no separate name), joined client-side against the
 * registry `GET /v1/repositories` already keeps warm. The `repository` filter
 * is the daemon's own — `GET /v1/permissions/learned?repository=<id>` — kept
 * in the URL the way every other screen's filters are, so a narrowed list
 * survives a reload.
 *
 * A row opens the same detail every field of it deserves
 * (`learned-permission-detail.tsx`); its own Edit and Delete stop that click
 * from reaching the row underneath, the way every other table's row actions do
 * (`features/sessions/sessions-list.tsx`).
 */

import { useQuery } from "@tanstack/react-query"
import { PencilIcon, PlusIcon, ShieldCheckIcon, Trash2Icon } from "lucide-react"
import { useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"

import type { LearnedPermissionDto, LearnedPermissionLabel, LearnedPermissionSource } from "@/api"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { Button } from "@/components/ui/button"
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
import { plural } from "@/lib/format"

import { DeleteLearnedPermissionDialog } from "./delete-learned-permission-dialog"
import { LearnedPermissionDetail } from "./learned-permission-detail"
import { LearnedPermissionFormDialog } from "./learned-permission-form-dialog"
import { learnedPermissionsQueryOptions } from "./queries"

/** The param the repository filter travels in, the daemon's own name for it. */
const REPOSITORY_PARAM = "repository"

/** No filter, in the Select: the value an absent param stands for. */
const ALL = "all"

const SOURCE_LABELS: Record<LearnedPermissionSource, string> = {
  console: "Console",
  manual: "Manual",
}

const AI_LABELS: Record<LearnedPermissionLabel, string> = {
  allow: "Allow",
  ask: "Ask",
  deny: "Deny",
}

const COLUMNS = [
  { header: "Repository" },
  { header: "Tool" },
  { header: "Kind" },
  { header: "Source" },
  { header: "AI" },
  { header: "Learned" },
  { className: "w-24 text-right" },
]

export function LearnedPermissionsTab() {
  const [search, setSearch] = useSearchParams()
  const repositoryFilter = search.get(REPOSITORY_PARAM) ?? undefined

  const repositories = useQuery(repositoriesQueryOptions())
  const learned = useQuery(learnedPermissionsQueryOptions({ repository: repositoryFilter }))

  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<LearnedPermissionDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<LearnedPermissionDto | null>(null)
  const [detailId, setDetailId] = useState<string | null>(null)

  const pathById = useMemo(
    () => new Map((repositories.data ?? []).map((repository) => [repository.id, repository.path])),
    [repositories.data],
  )
  const repositoryItems = useMemo(
    () => [
      { value: ALL, label: "All repositories" },
      ...(repositories.data ?? []).map((repository) => ({
        value: repository.id,
        label: repository.path,
      })),
    ],
    [repositories.data],
  )

  function setRepositoryFilter(value: string | null) {
    const next = new URLSearchParams(search)
    if (value === null || value === ALL) next.delete(REPOSITORY_PARAM)
    else next.set(REPOSITORY_PARAM, value)
    setSearch(next, { replace: true })
  }

  function openCreate() {
    setEditing(null)
    setFormOpen(true)
  }

  function openEdit(row: LearnedPermissionDto) {
    setEditing(row)
    setFormOpen(true)
  }

  function openDelete(row: LearnedPermissionDto) {
    setDeleting(row)
    setDeleteOpen(true)
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Select
          value={repositoryFilter ?? ALL}
          onValueChange={setRepositoryFilter}
          items={repositoryItems}
        >
          <SelectTrigger aria-label="Filter by repository" className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent alignItemWithTrigger={false}>
            <SelectItem value={ALL}>All repositories</SelectItem>
            {(repositories.data ?? []).map((repository) => (
              <SelectItem key={repository.id} value={repository.id}>
                {repository.path}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Button onClick={openCreate}>
          <PlusIcon />
          Add approval
        </Button>
      </div>

      {learned.data ? (
        <p className="text-sm text-muted-foreground">{plural(learned.data.length, "approval")}</p>
      ) : null}

      <DataTable
        query={learned}
        errorTitle="Could not load learned approvals"
        columns={COLUMNS}
        empty={
          <NoLearnedPermissions filtered={repositoryFilter !== undefined} onCreate={openCreate} />
        }
        rowKey={(row) => row.id}
        renderRow={(row) => (
          <LearnedPermissionRow
            row={row}
            repositoryPath={pathById.get(row.repository_id) ?? row.repository_id}
            onOpen={() => setDetailId(row.id)}
            onEdit={() => openEdit(row)}
            onDelete={() => openDelete(row)}
          />
        )}
      />

      <LearnedPermissionFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        editing={editing}
        repositories={repositories.data ?? []}
      />
      <DeleteLearnedPermissionDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        learned={deleting}
      />
      {detailId ? (
        <LearnedPermissionDetail id={detailId} onClose={() => setDetailId(null)} />
      ) : null}
    </div>
  )
}

function LearnedPermissionRow({
  row,
  repositoryPath,
  onOpen,
  onEdit,
  onDelete,
}: {
  row: LearnedPermissionDto
  repositoryPath: string
  onOpen: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  return (
    <TableRow
      className="cursor-pointer"
      // Anywhere on the row opens the detail view; the row actions carry a
      // click of their own and must not also open it.
      onClick={(event) => {
        const target = event.target as Element
        if (event.currentTarget.contains(target) && !target.closest("button")) onOpen()
      }}
    >
      <TableCell className="max-w-36 truncate font-mono text-xs lg:max-w-72" title={repositoryPath}>
        {repositoryPath}
      </TableCell>
      <TableCell className="font-mono text-xs">{row.tool_name}</TableCell>
      <TableCell className="text-xs text-muted-foreground">{row.kind}</TableCell>
      <TableCell className="text-xs">{SOURCE_LABELS[row.source]}</TableCell>
      <TableCell className="text-xs">
        {row.label ? (
          <>
            {AI_LABELS[row.label]}
            {row.danger !== null && row.danger !== undefined ? (
              <span className="text-muted-foreground"> · {row.danger.toFixed(2)}</span>
            ) : null}
          </>
        ) : (
          <span className="text-muted-foreground">—</span>
        )}
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        <When at={row.created_at} format="age" label="learned" />
      </TableCell>
      <TableCell className="text-right">
        <RowAction icon={<PencilIcon />} label={`Edit ${row.tool_name}`} onClick={onEdit} />
        <RowAction icon={<Trash2Icon />} label={`Remove ${row.tool_name}`} onClick={onDelete} />
      </TableCell>
    </TableRow>
  )
}

function NoLearnedPermissions({ filtered, onCreate }: { filtered: boolean; onCreate: () => void }) {
  return (
    <EmptyState
      className="border-0 py-12"
      icon={<ShieldCheckIcon className="size-8" />}
      title={filtered ? "No approvals for this repository" : "No learned approvals yet"}
      description={
        filtered
          ? "Pick another repository, or clear the filter."
          : "An approval is remembered the first time a console pick says to, or added here by hand."
      }
      action={
        !filtered ? (
          <Button variant="outline" size="sm" onClick={onCreate}>
            <PlusIcon />
            Add approval
          </Button>
        ) : undefined
      }
    />
  )
}
