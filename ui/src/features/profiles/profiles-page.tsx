/**
 * The profiles screen: `ariadne profile ls | inspect | create | update | rm` as
 * one table.
 *
 * Rows expand into the full profile instead of linking away, because the only
 * field that does not fit a table — the system prompt — is also the one worth
 * reading next to the others.
 *
 * Nothing here polls: the list is a plain query and the SSE dispatcher
 * invalidates it, so a profile created from the CLI shows up on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronRightIcon, PencilIcon, PlusIcon, Trash2Icon } from "lucide-react"
import { Fragment, type ReactNode, useState } from "react"

import { ApiError, type ProfileDto, type Role } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
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
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ROLE_LABELS } from "@/lib/labels"
import { formatAbsolute } from "@/lib/time"
import { cn } from "@/lib/utils"

import { DeleteProfileDialog } from "./delete-profile-dialog"
import { ProfileDetails } from "./profile-details"
import { ProfileFormDialog } from "./profile-form-dialog"
import { agentKindLabel, modelLabel, ROLES } from "./profile-labels"
import { profilesQueryOptions } from "./queries"

/** The role tabs, where "all" means the unfiltered request. */
type RoleFilter = Role | "all"

const COLUMN_COUNT = 6

export function ProfilesPage() {
  const [roleFilter, setRoleFilter] = useState<RoleFilter>("all")
  const [expandedId, setExpandedId] = useState<string | null>(null)

  // The dialogs keep their subject after closing so the exit animation still
  // has something to render; only `open` flips on close.
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<ProfileDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<ProfileDto | null>(null)

  const profiles = useQuery(profilesQueryOptions(roleFilter === "all" ? undefined : roleFilter))

  function openCreate() {
    setEditing(null)
    setFormOpen(true)
  }

  function openEdit(profile: ProfileDto) {
    setEditing(profile)
    setFormOpen(true)
  }

  function openDelete(profile: ProfileDto) {
    setDeleting(profile)
    setDeleteOpen(true)
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Profiles"
        description="What an agent runs as: a role in the orchestration, an agent CLI and model, and the system prompt it is spawned with."
        actions={
          <Button onClick={openCreate}>
            <PlusIcon />
            New profile
          </Button>
        }
      />

      <div className="flex flex-wrap items-center justify-between gap-3">
        <Tabs value={roleFilter} onValueChange={(value) => setRoleFilter(value as RoleFilter)}>
          <TabsList>
            <TabsTrigger value="all">All</TabsTrigger>
            {ROLES.map((role) => (
              <TabsTrigger key={role} value={role}>
                {ROLE_LABELS[role]}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {profiles.data ? (
          <p className="text-sm text-muted-foreground">
            {profiles.data.length} {profiles.data.length === 1 ? "profile" : "profiles"}
          </p>
        ) : null}
      </div>

      {profiles.isError ? (
        <ErrorState
          title="Could not load profiles"
          error={profiles.error}
          // A daemon that never answered has nothing to say about why.
          description={
            ApiError.is(profiles.error) && profiles.error.isNetworkError
              ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
              : undefined
          }
          onRetry={() => void profiles.refetch()}
        />
      ) : (
        <div className="overflow-hidden rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Name</TableHead>
                <TableHead>Role</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead>Model</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead className="w-20 text-right">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {profiles.isPending ? (
                <LoadingRows />
              ) : profiles.data.length === 0 ? (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={COLUMN_COUNT} className="p-0">
                    <NoProfiles roleFilter={roleFilter} onCreate={openCreate} />
                  </TableCell>
                </TableRow>
              ) : (
                profiles.data.map((profile) => (
                  <ProfileRow
                    key={profile.id}
                    profile={profile}
                    expanded={expandedId === profile.id}
                    onToggle={() =>
                      setExpandedId((current) => (current === profile.id ? null : profile.id))
                    }
                    onEdit={() => openEdit(profile)}
                    onDelete={() => openDelete(profile)}
                  />
                ))
              )}
            </TableBody>
          </Table>
        </div>
      )}

      <ProfileFormDialog open={formOpen} onOpenChange={setFormOpen} profile={editing} />
      <DeleteProfileDialog open={deleteOpen} onOpenChange={setDeleteOpen} profile={deleting} />
    </div>
  )
}

function ProfileRow({
  profile,
  expanded,
  onToggle,
  onEdit,
  onDelete,
}: {
  profile: ProfileDto
  expanded: boolean
  onToggle: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const detailsId = `profile-details-${profile.id}`

  return (
    <Fragment>
      <TableRow className={cn(expanded && "border-b-0")}>
        <TableCell className="font-medium">
          <Button
            variant="ghost"
            size="sm"
            className="-ml-1.5 font-medium"
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={onToggle}
          >
            <ChevronRightIcon
              className={cn("transition-transform duration-150", expanded && "rotate-90")}
            />
            {profile.name}
          </Button>
        </TableCell>
        <TableCell>
          <Badge variant="secondary">{ROLE_LABELS[profile.role]}</Badge>
        </TableCell>
        <TableCell>
          <Unset when={!profile.agent_kind}>{agentKindLabel(profile.agent_kind)}</Unset>
        </TableCell>
        <TableCell className="font-mono text-xs">
          <Unset when={!profile.model}>{modelLabel(profile.model)}</Unset>
        </TableCell>
        <TableCell className="text-muted-foreground">
          {formatAbsolute(profile.updated_at)}
        </TableCell>
        <TableCell className="text-right">
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Edit ${profile.name}`}
            title={`Edit ${profile.name}`}
            onClick={onEdit}
          >
            <PencilIcon />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Delete ${profile.name}`}
            title={`Delete ${profile.name}`}
            onClick={onDelete}
          >
            <Trash2Icon />
          </Button>
        </TableCell>
      </TableRow>
      {expanded ? (
        <TableRow className="hover:bg-transparent">
          <TableCell id={detailsId} colSpan={COLUMN_COUNT} className="whitespace-normal px-4 pt-0">
            <ProfileDetails profile={profile} />
          </TableCell>
        </TableRow>
      ) : null}
    </Fragment>
  )
}

/** A value the daemon does not hold, spelled out instead of left blank. */
function Unset({ when, children }: { when: boolean; children: ReactNode }) {
  return when ? <span className="text-muted-foreground italic">{children}</span> : children
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

function NoProfiles({ roleFilter, onCreate }: { roleFilter: RoleFilter; onCreate: () => void }) {
  const filtered = roleFilter !== "all"
  return (
    <EmptyState
      // The table's own frame is the box here.
      className="border-0 py-12"
      title={filtered ? `No ${ROLE_LABELS[roleFilter].toLowerCase()} profiles` : "No profiles yet"}
      description={
        filtered
          ? "Goals need a planner, and every task needs an engineer and at least one reviewer."
          : "Profiles are what goals and tasks are assigned to. Create the first one here, or with ariadne profile create."
      }
      action={
        <Button variant="outline" size="sm" onClick={onCreate}>
          <PlusIcon />
          New profile
        </Button>
      }
    />
  )
}
