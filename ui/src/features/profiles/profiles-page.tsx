/**
 * The profiles screen: `ariadne profile ls | inspect | create | update | rm` as
 * one table.
 *
 * Rows expand into the full profile instead of linking away, because the only
 * field that does not fit a table — the system prompt — is also the one worth
 * reading next to the others. A link to one profile is therefore a link to this
 * screen with `?expand=<id>` on it (`paths.profile`), which is what the command
 * palette takes a picked profile to.
 *
 * Both of this screen's pieces of view state live in the URL — the expanded row
 * under that same `?expand=`, the role tab under `?role=` — the way every
 * sibling surface keeps its own (the sessions screen's filters, the panels'
 * tabs). A hash-router desktop app reloads often enough that component state
 * would silently drop them, and the expansion is the thing links point at.
 * The tab replaces, the way the sessions screen's filters do — a filter is not
 * a place — while an expansion pushes, so Back closes the row it opened.
 *
 * Nothing here polls: the list is a plain query and the SSE dispatcher
 * invalidates it, so a profile created from the CLI shows up on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronRightIcon, PencilIcon, PlusIcon, Trash2Icon } from "lucide-react"
import { Fragment, type ReactNode, useCallback, useEffect, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"

import type { ProfileDto, Role } from "@/api"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { cn, plural, ROLE_LABELS } from "@/lib/format"
import { PROFILE_EXPAND_PARAM } from "@/routes/paths"

import { DeleteProfileDialog } from "./delete-profile-dialog"
import { ProfileDetails } from "./profile-details"
import { ProfileFormDialog } from "./profile-form-dialog"
import { agentKindLabel, modelLabel, ROLES } from "./profile-labels"
import { profilesQueryOptions } from "./queries"

/** The role tabs, where "all" means the unfiltered request. */
type RoleFilter = Role | "all"

/** The param the role tab travels in, alongside `?expand=`. */
const ROLE_PARAM = "role"

/** No filter, on the tab strip: the value an absent `?role=` stands for. */
const ALL = "all"

const COLUMNS = [
  { header: "Name" },
  { header: "Role" },
  { header: "Agent" },
  { header: "Model" },
  { header: "Updated" },
  { className: "w-20 text-right" },
]

export function ProfilesPage() {
  // The dialogs keep their subject after closing so the exit animation still
  // has something to render; only `open` flips on close.
  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState<ProfileDto | null>(null)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<ProfileDto | null>(null)

  const [search, setSearch] = useSearchParams()
  const roleFilter: RoleFilter = ROLES.find((role) => role === search.get(ROLE_PARAM)) ?? ALL
  const expandedId = search.get(PROFILE_EXPAND_PARAM)

  const profiles = useQuery(profilesQueryOptions(roleFilter === ALL ? undefined : roleFilter))

  /** The row a link asked for, until it has been scrolled to. */
  const [scrollToId, setScrollToId] = useState<string | null>(null)
  /** The last row expanded by a click here, which is already on screen. */
  const clicked = useRef<string | null>(null)

  /**
   * Anything that expands a row *other* than a click on it — a link followed
   * onto this screen (`paths.profile`, which is where the command palette
   * takes a picked profile), a reload, a Back step — has to bring the row into
   * view, since there is no reason for it to be where the user is looking.
   */
  useEffect(() => {
    if (!expandedId) {
      clicked.current = null
      return
    }
    if (expandedId !== clicked.current) setScrollToId(expandedId)
  }, [expandedId])

  /**
   * Selects a role, keeping every other param. It replaces rather than pushes,
   * the way the sessions screen's filters do: a filter is not a place, and
   * Back should leave this screen rather than walk the tabs that got here.
   *
   * The expansion rides along, so widening the tab again shows the open row
   * rather than having lost it. The rule the other way round — that a linked
   * profile is never filtered out of the list it lands in — belongs to the URL
   * now: `paths.profile` is a whole search string, with an id and no role on
   * it, so following one drops whatever tab was up.
   */
  function filterByRole(value: string) {
    const next = new URLSearchParams(search)
    if (value === ALL) next.delete(ROLE_PARAM)
    else next.set(ROLE_PARAM, value)
    setSearch(next, { replace: true })
  }

  /**
   * Opens a row, or closes the open one. Unlike the tab, this pushes: an
   * expansion is what a link points at, so Back has to close the row it opened
   * and Forward has to bring it back.
   */
  function toggleExpanded(profileId: string) {
    const next = new URLSearchParams(search)
    if (expandedId === profileId) {
      clicked.current = null
      next.delete(PROFILE_EXPAND_PARAM)
    } else {
      clicked.current = profileId
      next.set(PROFILE_EXPAND_PARAM, profileId)
    }
    setSearch(next)
  }

  /**
   * Scrolls the asked-for row into view as it mounts — which is the first
   * render for a link followed from another screen, and a later one when the
   * list is still loading.
   */
  const scrollToRow = useCallback((row: HTMLTableRowElement | null) => {
    if (!row) return
    row.scrollIntoView({ block: "center", behavior: "smooth" })
    setScrollToId(null)
  }, [])

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
        <Tabs value={roleFilter} onValueChange={filterByRole}>
          <TabsList>
            <TabsTrigger value={ALL}>All</TabsTrigger>
            {ROLES.map((role) => (
              <TabsTrigger key={role} value={role}>
                {ROLE_LABELS[role]}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
        {profiles.data ? (
          <p className="text-sm text-muted-foreground">{plural(profiles.data.length, "profile")}</p>
        ) : null}
      </div>

      <DataTable
        query={profiles}
        errorTitle="Could not load profiles"
        columns={COLUMNS}
        empty={<NoProfiles roleFilter={roleFilter} onCreate={openCreate} />}
        rowKey={(profile) => profile.id}
        renderRow={(profile) => (
          <ProfileRow
            profile={profile}
            ref={profile.id === scrollToId ? scrollToRow : undefined}
            expanded={expandedId === profile.id}
            onToggle={() => toggleExpanded(profile.id)}
            onEdit={() => openEdit(profile)}
            onDelete={() => openDelete(profile)}
          />
        )}
      />

      <ProfileFormDialog open={formOpen} onOpenChange={setFormOpen} profile={editing} />
      <DeleteProfileDialog open={deleteOpen} onOpenChange={setDeleteOpen} profile={deleting} />
    </div>
  )
}

function ProfileRow({
  profile,
  ref,
  expanded,
  onToggle,
  onEdit,
  onDelete,
}: {
  profile: ProfileDto
  /** Set on the row a link asked for, so the screen can scroll to it. */
  ref?: (row: HTMLTableRowElement | null) => void
  expanded: boolean
  onToggle: () => void
  onEdit: () => void
  onDelete: () => void
}) {
  const detailsId = `profile-details-${profile.id}`

  return (
    <Fragment>
      <TableRow ref={ref} className={cn(expanded && "border-b-0")}>
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
        {/* The age is what a table is read for; the full stamp is the hint
            behind it, the same way every other timestamp in the app shows it. */}
        <TableCell className="text-muted-foreground">
          <When at={profile.updated_at} label="updated" />
        </TableCell>
        <TableCell className="text-right">
          <RowAction icon={<PencilIcon />} label={`Edit ${profile.name}`} onClick={onEdit} />
          <RowAction icon={<Trash2Icon />} label={`Delete ${profile.name}`} onClick={onDelete} />
        </TableCell>
      </TableRow>
      {expanded ? (
        <TableRow className="hover:bg-transparent">
          <TableCell
            id={detailsId}
            colSpan={COLUMNS.length}
            className="whitespace-normal px-4 pt-0"
          >
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

function NoProfiles({ roleFilter, onCreate }: { roleFilter: RoleFilter; onCreate: () => void }) {
  const filtered = roleFilter !== ALL
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
