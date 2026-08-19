/**
 * The repositories screen: the checkouts Ariadne knows about, as one table.
 *
 * A repository is three fields wide, so the rows do not expand — everything a
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

import { ApiError, type RepositoryDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { plural } from "@/lib/plural"

import { DeleteRepositoryDialog } from "./delete-repository-dialog"
import { repositoriesQueryOptions } from "./queries"
import { RepositoryFormDialog } from "./repository-form-dialog"

const COLUMN_COUNT = 4

export function RepositoriesPage() {
  // The dialogs keep their subject after closing so the exit animation still
  // has something to render; only `open` flips on close.
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<RepositoryDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<RepositoryDto | null>(null)

  const repositories = useQuery(repositoriesQueryOptions())

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
        description="The git checkouts goals are created against. Each one is a path, the branch task worktrees are cut from, and what it is for."
        actions={
          <Button onClick={openCreate}>
            <PlusIcon />
            Register repository
          </Button>
        }
      />

      {repositories.data ? (
        <p className="text-sm text-muted-foreground">
          {plural(repositories.data.length, "repository", "repositories")}
        </p>
      ) : null}

      {repositories.isError ? (
        <ErrorState
          title="Could not load repositories"
          error={repositories.error}
          // A daemon that never answered has nothing to say about why.
          description={
            ApiError.is(repositories.error) && repositories.error.isNetworkError
              ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
              : undefined
          }
          onRetry={() => void repositories.refetch()}
        />
      ) : (
        <div className="overflow-hidden rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Path</TableHead>
                <TableHead>Base branch</TableHead>
                <TableHead>Description</TableHead>
                <TableHead className="w-20 text-right">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {repositories.isPending ? (
                <LoadingRows />
              ) : repositories.data.length === 0 ? (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={COLUMN_COUNT} className="p-0">
                    <NoRepositories onCreate={openCreate} />
                  </TableCell>
                </TableRow>
              ) : (
                repositories.data.map((repository) => (
                  <RepositoryRow
                    key={repository.id}
                    repository={repository}
                    onEdit={() => openEdit(repository)}
                    onDelete={() => openDelete(repository)}
                  />
                ))
              )}
            </TableBody>
          </Table>
        </div>
      )}

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
  onEdit,
  onDelete,
}: {
  repository: RepositoryDto
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
          the longest path there is. */}
      <TableCell className="max-w-96 text-xs font-medium">
        <CopyableId value={repository.path} label="repository path" truncate="middle" />
      </TableCell>
      <TableCell className="max-w-56 text-xs">
        <CopyableId value={repository.base_branch} label="base branch" truncate="middle" />
      </TableCell>
      <TableCell className="whitespace-normal text-muted-foreground">
        {repository.description ?? <span className="italic">no description</span>}
      </TableCell>
      <TableCell className="text-right">
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Edit ${repository.path}`}
          title={`Edit ${repository.path}`}
          onClick={onEdit}
        >
          <PencilIcon />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Remove ${repository.path}`}
          title={`Remove ${repository.path}`}
          onClick={onDelete}
        >
          <Trash2Icon />
        </Button>
      </TableCell>
    </TableRow>
  )
}

function LoadingRows() {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row} className="hover:bg-transparent">
          {Array.from({ length: COLUMN_COUNT }, (_, column) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells have no identity
            <TableCell key={column}>
              <Skeleton className="h-4 w-full" />
            </TableCell>
          ))}
        </TableRow>
      ))}
    </>
  )
}

function NoRepositories({ onCreate }: { onCreate: () => void }) {
  return (
    <EmptyState
      // The table's own frame is the box here.
      className="border-0 py-12"
      title="No repositories yet"
      description="A goal is created against registered repositories, so register the first one here, or with ariadne repo add."
      action={
        <Button variant="outline" size="sm" onClick={onCreate}>
          <PlusIcon />
          Register repository
        </Button>
      }
    />
  )
}
