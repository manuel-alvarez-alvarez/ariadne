/**
 * The repositories screen: the checkouts Ariadne knows about, as one table.
 *
 * A repository is four fields wide, so the rows do not expand — everything a
 * repository is is already in the row, and the edit dialog is where the rest of
 * it happens. That is the one deliberate difference from the profiles screen
 * this otherwise follows.
 *
 * Goals reference these live: a goal is created against registered
 * repositories, and an edit here shows up in every goal that works in one.
 * Nothing polls — the SSE dispatcher invalidates the list, so a repository
 * registered from the CLI shows up on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { PencilIcon, PlusIcon, Trash2Icon } from "lucide-react"
import { useState } from "react"

import type { ForgeTunnelDto, RepositoryDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { DataTable, RowAction } from "@/components/data-table"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"

import { DeleteRepositoryDialog } from "./delete-repository-dialog"
import { forgeKindLabel, forgeRepositoryLabel } from "./forge"
import { NoRepositories as SharedNoRepositories } from "./no-repositories"
import { permissionModeLabel } from "./permission-modes"
import { repositoriesQueryOptions, tunnelQueryOptions } from "./queries"
import { RepositoryFormDialog } from "./repository-form-dialog"
import { WebhookPill } from "./webhook-pill"

const COLUMNS = [
  { header: "Path" },
  { header: "Base branch" },
  { header: "Permissions" },
  { header: "Forge" },
  { header: "Webhook", className: "w-28" },
  // Wide enough to be a sentence rather than a word per line: what made the
  // rows of this table 130px tall was a description with nothing to wrap in.
  { header: "Description", className: "min-w-48" },
  { className: "w-28 text-right" },
]

export function RepositoriesPage() {
  // The dialogs keep their subject after closing so the exit animation still
  // has something to render; only `open` flips on close.
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<RepositoryDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<RepositoryDto | null>(null)

  const repositories = useQuery(repositoriesQueryOptions())
  // Whether a polling hook is the tunnel being off or a hook that failed.
  const tunnel = useQuery(tunnelQueryOptions())

  function openCreate() {
    setEditing(null)
    setFormOpen(true)
  }

  function openEdit(repository: RepositoryDto) {
    setEditing(repository)
    setFormOpen(true)
  }

  function openDelete(repository: RepositoryDto) {
    setDeleting(repository)
    setDeleteOpen(true)
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Repositories"
        description="The git checkouts goals are created against. Each one is a path, the branch task worktrees are cut from, how its agents' permission requests are answered, the forge its remote is on, and what it is for."
        actions={
          <Button onClick={openCreate}>
            <PlusIcon />
            Register repository
          </Button>
        }
      />

      <DataTable
        query={repositories}
        errorTitle="Could not load repositories"
        columns={COLUMNS}
        empty={<NoRepositories onCreate={openCreate} />}
        rowKey={(repository) => repository.id}
        renderRow={(repository) => (
          <RepositoryRow
            repository={repository}
            tunnel={tunnel.data}
            onEdit={() => openEdit(repository)}
            onDelete={() => openDelete(repository)}
          />
        )}
      />

      <RepositoryFormDialog open={formOpen} onOpenChange={setFormOpen} repository={editing} />
      <DeleteRepositoryDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        repository={deleting}
      />
    </div>
  )
}

function RepositoryRow({
  repository,
  tunnel,
  onEdit,
  onDelete,
}: {
  repository: RepositoryDto
  tunnel: ForgeTunnelDto | undefined
  onEdit: () => void
  onDelete: () => void
}) {
  return (
    <TableRow>
      {/* The path is what this screen is visited for: it is read here on its
          way into a terminal, so it is one click onto the clipboard rather
          than a retype, and the ellipsis goes in the middle — the checkout's
          own name is the last segment, which cutting the end would take.
          `max-w-*` on the cell is what makes the ellipsis possible at all: the
          cell's own `whitespace-nowrap` would otherwise widen the column to
          the longest path there is. It is a hard cap below `lg`, where the
          full width the path would take is the width the description needs to
          be prose. */}
      <TableCell className="max-w-36 text-xs font-medium lg:max-w-96">
        <CopyableId value={repository.path} label="repository path" truncate="middle" />
      </TableCell>
      <TableCell className="max-w-24 text-xs lg:max-w-56">
        <CopyableId value={repository.base_branch} label="base branch" truncate="middle" />
      </TableCell>
      <TableCell className="text-xs">{permissionModeLabel(repository.permission_mode)}</TableCell>
      <TableCell className="text-xs">
        {repository.forge ? (
          <span className="flex flex-col" title={forgeRepositoryLabel(repository.forge)}>
            <span>
              {forgeKindLabel(repository.forge.kind)}
              {repository.forge.enabled ? null : (
                <span className="text-muted-foreground"> · off</span>
              )}
            </span>
            <span className="font-mono text-muted-foreground">
              {repository.forge.owner}/{repository.forge.name}
            </span>
          </span>
        ) : (
          <span className="text-muted-foreground">none</span>
        )}
      </TableCell>
      <TableCell className="text-xs">
        {repository.forge ? (
          <WebhookPill forge={repository.forge} tunnel={tunnel} />
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell className="min-w-48 whitespace-normal text-muted-foreground">
        {repository.description ?? <span className="italic">no description</span>}
      </TableCell>
      <TableCell className="text-right">
        <RowAction icon={<PencilIcon />} label={`Edit ${repository.path}`} onClick={onEdit} />
        <RowAction icon={<Trash2Icon />} label={`Remove ${repository.path}`} onClick={onDelete} />
      </TableCell>
    </TableRow>
  )
}

function NoRepositories({ onCreate }: { onCreate: () => void }) {
  return (
    <SharedNoRepositories
      // The table's own frame is the box here.
      className="border-0 py-12"
      // This is the screen that registers them, so the way out is the form.
      action={
        <Button variant="outline" size="sm" onClick={onCreate}>
          <PlusIcon />
          Register repository
        </Button>
      }
    />
  )
}
