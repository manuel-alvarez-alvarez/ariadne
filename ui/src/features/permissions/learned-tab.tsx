/**
 * The Learned tab: every choice a repository's agents made, or had made for
 * them — a console pick under `learn` or `ai`, or a denial the AI model
 * reached on its own — kept so a future run of the same request answers the
 * same way and so the choices can train the model that makes them.
 *
 * A repository is shown by its folder name — the last path segment, what a
 * person actually recognises it by — with the full path kept as a `title` and,
 * in the filter's own popup, as a muted second line under the name
 * (`RepositoryDto` carries no separate name to show instead). Joined
 * client-side against the registry `GET /v1/repositories` already keeps warm.
 * The `repository` filter is the daemon's own —
 * `GET /v1/permissions/learned?repository=<id>` — kept in the URL the way
 * every other screen's filters are, so a narrowed list survives a reload; an
 * id the registry no longer carries still shows a readable trigger rather
 * than an empty one, since the filter reads the raw value instead of trusting
 * a matching option to exist.
 *
 * A row opens the same detail every field of it deserves
 * (`learned-permission-detail.tsx`); its own Delete stops that click
 * from reaching the row underneath, the way every other table's row actions do
 * (`features/sessions/sessions-list.tsx`). The row is a Tab stop of its own
 * too, opening the same detail on Enter or Space, since a click is not the
 * only way in.
 *
 * The actions column is pinned to the trailing edge (`features/models/model-
 * table.tsx`'s pattern): a table wide enough to scroll must not carry Remove
 * off the screen with the columns that are only data.
 */

import { useQuery } from "@tanstack/react-query"
import { ShieldCheckIcon, Trash2Icon } from "lucide-react"
import { useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"

import type { LearnedPermissionDto } from "@/api"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { StatusBadge } from "@/components/status-badge"
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
import { permissionModeLabel } from "@/features/repositories/permission-modes"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { cn, folderName, shortId } from "@/lib/format"

import { AI_LABEL_TEXT, AI_LABEL_TONE, parseAiOutput } from "./ai-output"
import { DeleteLearnedPermissionDialog } from "./delete-learned-permission-dialog"
import { LearnedPermissionDetail } from "./learned-permission-detail"
import { learnedPermissionsQueryOptions } from "./queries"
import { selectedOptionInfo } from "./request-summary"

/** The param the repository filter travels in, the daemon's own name for it. */
const REPOSITORY_PARAM = "repository"

/** No filter, in the Select: the value an absent param stands for. */
const ALL = "all"

/** The pinned actions column, held against the trailing edge — see `model-table.tsx`. */
const PINNED = "sticky right-0 z-20 bg-inherit pe-3"

const OUTCOME_TONE = {
  allow: "bg-status-done-soft text-status-done-fg",
  deny: "bg-status-danger-soft text-status-danger-fg",
} as const

const COLUMNS = [
  { header: "Tool" },
  { header: "Level" },
  { header: "Family" },
  { header: "Key" },
  { header: "Scope" },
  { header: "Target" },
  { header: "Selected option" },
  { header: "AI score" },
  { header: "Created" },
  { header: "Updated" },
  { className: cn("w-16 text-right", PINNED) },
]

export function LearnedPermissionsTab() {
  const [search, setSearch] = useSearchParams()
  const repositoryFilter = search.get(REPOSITORY_PARAM) ?? undefined

  const repositories = useQuery(repositoriesQueryOptions())
  const learned = useQuery(learnedPermissionsQueryOptions({ repository: repositoryFilter }))

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
        label: folderName(repository.path),
      })),
    ],
    [repositories.data],
  )

  // The raw param, not the matching item: a repository the registry no longer
  // carries (deleted, or a link built from a stale list) must still show
  // something a reader recognises rather than a trigger with nothing in it.
  const selectedRepositoryPath = repositoryFilter ? pathById.get(repositoryFilter) : undefined
  const filterTriggerLabel = !repositoryFilter
    ? "All repositories"
    : selectedRepositoryPath
      ? folderName(selectedRepositoryPath)
      : shortId(repositoryFilter)
  const filterTriggerTitle = repositoryFilter
    ? (selectedRepositoryPath ?? repositoryFilter)
    : undefined

  function setRepositoryFilter(value: string | null) {
    const next = new URLSearchParams(search)
    if (value === null || value === ALL) next.delete(REPOSITORY_PARAM)
    else next.set(REPOSITORY_PARAM, value)
    setSearch(next, { replace: true })
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
          <SelectTrigger
            aria-label="Filter by repository"
            className="w-48"
            title={filterTriggerTitle}
          >
            <SelectValue>{filterTriggerLabel}</SelectValue>
          </SelectTrigger>
          {/* Wide enough for a full path, and not tied to the trigger's own
              narrow width — see `select.tsx`'s `w-(--anchor-width)` default. */}
          <SelectContent alignItemWithTrigger={false} className="w-80">
            <SelectItem value={ALL}>All repositories</SelectItem>
            {(repositories.data ?? []).map((repository) => (
              <SelectItem key={repository.id} value={repository.id} aria-label={repository.path}>
                <span className="flex min-w-0 flex-col overflow-hidden py-0.5">
                  <span className="truncate">{folderName(repository.path)}</span>
                  <span className="truncate text-xs text-muted-foreground">{repository.path}</span>
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <DataTable
        query={learned}
        errorTitle="Could not load the learned rows"
        columns={COLUMNS}
        pinnedEnd
        empty={
          <NoLearnedPermissions
            filtered={repositoryFilter !== undefined}
            onClearFilter={() => setRepositoryFilter(null)}
          />
        }
        rowKey={(row) => row.id}
        renderRow={(row) => (
          <LearnedPermissionRow
            row={row}
            onOpen={() => setDetailId(row.id)}
            onDelete={() => openDelete(row)}
          />
        )}
      />

      <DeleteLearnedPermissionDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        learned={deleting}
        repositories={repositories.data ?? []}
      />
      {detailId ? (
        <LearnedPermissionDetail id={detailId} onClose={() => setDetailId(null)} />
      ) : null}
    </div>
  )
}

function LearnedPermissionRow({
  row,
  onOpen,
  onDelete,
}: {
  row: LearnedPermissionDto
  onOpen: () => void
  onDelete: () => void
}) {
  const targetLabel = permissionModeLabel(row.target)
  const option = selectedOptionInfo(row.options, row.selected_option)
  const ai = parseAiOutput(row.output)

  return (
    <TableRow
      tabIndex={0}
      aria-label={`${row.tool_name} (${targetLabel})`}
      className="cursor-pointer bg-background focus-visible:outline-none focus-visible:-outline-offset-2 focus-visible:ring-2 focus-visible:ring-ring/50"
      // Anywhere on the row opens the detail view; the row actions carry a
      // click of their own and must not also open it.
      onClick={(event) => {
        const target = event.target as Element
        if (event.currentTarget.contains(target) && !target.closest("button")) onOpen()
      }}
      onKeyDown={(event) => {
        // Only the row's own focus, not a keystroke bubbling up from Remove —
        // a button already answers Enter and Space for itself.
        if (event.target !== event.currentTarget) return
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault()
          onOpen()
        }
      }}
    >
      <TableCell className="max-w-24 truncate font-mono text-xs lg:max-w-40" title={row.tool_name}>
        {row.tool_name}
      </TableCell>
      <TableCell className="text-xs capitalize">{row.level}</TableCell>
      <TableCell className="max-w-28 truncate font-mono text-xs" title={row.family}>
        {row.family}
      </TableCell>
      <TableCell className="max-w-48 truncate font-mono text-xs" title={row.key}>
        {row.key}
      </TableCell>
      <TableCell className="text-xs">
        {row.scope === "all" ? "All repositories" : "This repository"}
      </TableCell>
      <TableCell className="text-xs">{targetLabel}</TableCell>
      <TableCell className="text-xs">
        {option.outcome ? (
          <StatusBadge size="sm" label={option.label} tone={OUTCOME_TONE[option.outcome]} />
        ) : (
          option.label
        )}
      </TableCell>
      <TableCell className="text-xs">
        {ai ? (
          <span className="flex flex-wrap items-center gap-1.5">
            {ai.label ? (
              <StatusBadge
                size="sm"
                label={AI_LABEL_TEXT[ai.label]}
                tone={AI_LABEL_TONE[ai.label]}
              />
            ) : null}
            {ai.allowProbability !== null ? (
              <span className="tabular-nums text-muted-foreground">
                allow {ai.allowProbability.toFixed(2)}
              </span>
            ) : null}
            {ai.denyProbability !== null ? (
              <span className="tabular-nums text-muted-foreground">
                deny {ai.denyProbability.toFixed(2)}
              </span>
            ) : null}
            {ai.danger !== null ? (
              <span className="tabular-nums text-muted-foreground">{ai.danger.toFixed(4)}</span>
            ) : null}
          </span>
        ) : (
          <span className="text-muted-foreground">—</span>
        )}
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        <When at={row.created_at} format="age" label="learned" />
      </TableCell>
      <TableCell className="text-xs text-muted-foreground">
        <When at={row.updated_at} format="age" label="updated" />
      </TableCell>
      <TableCell className={cn("text-right", PINNED)}>
        <RowAction icon={<Trash2Icon />} label={`Remove ${row.tool_name}`} onClick={onDelete} />
      </TableCell>
    </TableRow>
  )
}

function NoLearnedPermissions({
  filtered,
  onClearFilter,
}: {
  filtered: boolean
  onClearFilter: () => void
}) {
  return (
    <EmptyState
      className="border-0 py-12"
      icon={<ShieldCheckIcon className="size-8" />}
      title={filtered ? "No rows for this repository" : "Nothing learned yet"}
      description={
        filtered
          ? "Pick another repository, or clear the filter."
          : "A row is recorded the first time a repository's agents make a choice, or have one made for them."
      }
      action={
        filtered ? (
          <Button variant="outline" size="sm" onClick={onClearFilter}>
            Clear filter
          </Button>
        ) : undefined
      }
    />
  )
}
