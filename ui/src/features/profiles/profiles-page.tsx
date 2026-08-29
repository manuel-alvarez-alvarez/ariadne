/**
 * The profiles screen: `ariadne profile ls | inspect | create | update | rm`
 * as a list beside an editor.
 *
 * The list on the left is every profile under its role's heading — planner,
 * engineer, reviewer, in the order the orchestration runs them — and the pane
 * on the right is the selected one, edited in place (`profile-editor.tsx`).
 * A profile is mostly its prompts, and those are long: a table that folded
 * them into a row, and a dialog that stacked five of them, were both the wrong
 * shape for text that is read and rewritten a screen at a time.
 *
 * The selection lives in the URL, as `?profile=<id>` (`paths.profile`), the
 * way every sibling surface keeps its own view state: a link to a profile is
 * a link to this screen with the selection on it, which is where the command
 * palette takes a picked profile and where a profile's name links from
 * wherever it is mentioned. Selecting pushes, so Back returns to whatever was
 * selected before. The filter box above the list is not in the URL — it
 * narrows what is on screen and is not a place.
 *
 * Below `md` there is no room for both columns: the list is the screen, and a
 * selection shows the editor in its place with a way back to the list.
 *
 * Nothing here polls: the list is a plain query and the SSE dispatcher
 * invalidates it, so a profile created from the CLI shows up on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { ProfileDto, Role } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { cn, ROLE_LABELS } from "@/lib/format"
import { PROFILE_PARAM } from "@/routes/paths"

import { CreateProfileDialog } from "./create-profile-dialog"
import { pinLabel } from "./model-ref"
import { ProfileEditor } from "./profile-editor"
import { ROLES } from "./profile-labels"
import { profilesQueryOptions } from "./queries"

export function ProfilesPage() {
  const [createOpen, setCreateOpen] = useState(false)
  const [search, setSearch] = useSearchParams()
  const selectedId = search.get(PROFILE_PARAM)

  const profiles = useQuery(profilesQueryOptions())
  const selected = profiles.data?.find((profile) => profile.id === selectedId)

  /**
   * Selects a profile. It pushes: a selection is what a link points at, so
   * Back has to return to the one before it and Forward to bring it back.
   * Every other param is kept, so a panel open over the screen stays open.
   */
  function select(profileId: string) {
    const next = new URLSearchParams(search)
    next.set(PROFILE_PARAM, profileId)
    setSearch(next)
  }

  /**
   * Clears the selection in place. This is the way back to the list where the
   * list is not beside the editor, and where the selected profile is gone; in
   * neither case is the user going somewhere, so nothing is pushed.
   */
  function clearSelection() {
    const next = new URLSearchParams(search)
    next.delete(PROFILE_PARAM)
    setSearch(next, { replace: true })
  }

  return (
    // A fixed-height column against the shell's `<main>`, the way the board
    // is (see `goals-list-page.tsx`): the two columns below scroll on their
    // own, so the screen must not grow with them.
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Profiles"
        description="What an agent runs as: a role in the orchestration, the model that names the CLI running it, and every prompt it is spawned with."
        actions={
          <Button onClick={() => setCreateOpen(true)}>
            <PlusIcon />
            New profile
          </Button>
        }
      />

      {profiles.isPending ? (
        <LoadingProfiles />
      ) : profiles.isError ? (
        <ErrorState
          title="Could not load profiles"
          error={profiles.error}
          onRetry={() => void profiles.refetch()}
          showIcon
        />
      ) : profiles.data.length === 0 ? (
        <NoProfiles onCreate={() => setCreateOpen(true)} />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6">
          <ProfileList
            profiles={profiles.data}
            selectedId={selectedId}
            search={search}
            className={cn(selectedId ? "hidden md:flex" : "flex")}
          />
          <section
            aria-label="Selected profile"
            className={cn(
              "min-h-0 min-w-0 flex-1 flex-col",
              selectedId ? "flex" : "hidden md:flex",
            )}
          >
            {selected ? (
              // Keyed by the profile: a selection is a fresh editor, never one
              // profile's form refilled with another's.
              <ProfileEditor
                key={selected.id}
                profile={selected}
                onBack={clearSelection}
                onDeleted={clearSelection}
              />
            ) : selectedId ? (
              // A link to a profile that is gone — deleted since, or never
              // this daemon's — lands here rather than on nothing at all.
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="No profile by that id."
                description="It may have been deleted since the link was made."
                action={
                  <Button variant="outline" size="sm" onClick={clearSelection}>
                    Back to the list
                  </Button>
                }
              />
            ) : (
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="Select a profile, or create one."
              />
            )}
          </section>
        </div>
      )}

      <CreateProfileDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        // The new profile is on its role's own prompts, and the editor is
        // where those are rewritten: land there.
        onCreated={(profile) => select(profile.id)}
      />
    </div>
  )
}

/**
 * The list: every profile under its role, the selected one marked, and a box
 * above them to narrow by name.
 */
function ProfileList({
  profiles,
  selectedId,
  search,
  className,
}: {
  profiles: ProfileDto[]
  selectedId: string | null
  /** The screen's params, kept on every selection link. */
  search: URLSearchParams
  className?: string
}) {
  const [filter, setFilter] = useState("")
  const needle = filter.trim().toLowerCase()
  const shown = needle
    ? profiles.filter((profile) => profile.name.toLowerCase().includes(needle))
    : profiles
  const groups = ROLES.map((role) => ({
    role,
    profiles: shown.filter((profile) => profile.role === role),
  })).filter((group) => group.profiles.length > 0)

  /** The item a link asked for, until it has been scrolled to. */
  const [scrollToId, setScrollToId] = useState<string | null>(null)
  /** The last item selected by a click here, which is already on screen. */
  const clicked = useRef<string | null>(null)

  /**
   * Anything that selects a profile *other* than a click on it — a link
   * followed onto this screen (`paths.profile`, which is where the command
   * palette takes a picked profile), a reload, a Back step — has to bring the
   * item into view, since there is no reason for it to be where the user is
   * looking.
   */
  useEffect(() => {
    if (!selectedId) {
      clicked.current = null
      return
    }
    if (selectedId !== clicked.current) setScrollToId(selectedId)
  }, [selectedId])

  /**
   * Scrolls the asked-for item into view as it mounts — which is the first
   * render for a link followed from another screen, and a later one when the
   * list is still loading.
   */
  const scrollToItem = useCallback((item: HTMLAnchorElement | null) => {
    if (!item) return
    item.scrollIntoView({ block: "center", behavior: "smooth" })
    setScrollToId(null)
  }, [])

  /** The link that selects one profile, with every other param kept. */
  function selectionOf(profileId: string): { search: string } {
    const next = new URLSearchParams(search)
    next.set(PROFILE_PARAM, profileId)
    return { search: `?${next.toString()}` }
  }

  return (
    <aside className={cn("w-full min-w-0 flex-col gap-3 md:w-64 md:shrink-0", className)}>
      <Input
        type="search"
        aria-label="Filter profiles"
        placeholder="Filter by name…"
        autoComplete="off"
        spellCheck={false}
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      {/* The list's own scrollport; `contain-paint` for the same reason the
          editor's has it — what it hides must not scroll the shell. */}
      <nav
        aria-label="Profiles"
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto contain-paint"
      >
        {groups.length === 0 ? (
          <p className="px-2 text-sm text-muted-foreground">No profile is named that.</p>
        ) : (
          groups.map((group) => (
            <RoleGroup key={group.role} role={group.role}>
              {group.profiles.map((profile) => {
                const current = profile.id === selectedId
                return (
                  <li key={profile.id}>
                    <Link
                      to={selectionOf(profile.id)}
                      ref={profile.id === scrollToId ? scrollToItem : undefined}
                      aria-current={current ? "page" : undefined}
                      onClick={() => {
                        clicked.current = profile.id
                      }}
                      className={cn(
                        "flex flex-col rounded-md px-2 py-1.5 text-sm outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50",
                        current && "bg-muted font-medium",
                      )}
                    >
                      <span className="truncate">{profile.name}</span>
                      {/* What it runs on, after the name: the model, and
                          after an `@` the effort where one is pinned. */}
                      <span className="truncate font-mono text-xs font-normal text-muted-foreground">
                        {pinLabel(profile.model, profile.effort)}
                      </span>
                    </Link>
                  </li>
                )
              })}
            </RoleGroup>
          ))
        )}
      </nav>
    </aside>
  )
}

/** One role's profiles under its heading. */
function RoleGroup({ role, children }: { role: Role; children: React.ReactNode }) {
  const headingId = `profiles-${role}`
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-1">
      <h3
        id={headingId}
        className="px-2 text-xs font-medium tracking-wide text-muted-foreground uppercase"
      >
        {ROLE_LABELS[role]}
      </h3>
      <ul className="flex flex-col gap-0.5">{children}</ul>
    </section>
  )
}

/** Both columns, before the list has arrived. */
function LoadingProfiles() {
  return (
    <div className="flex min-h-0 flex-1 gap-6" aria-busy>
      <div className="flex w-full flex-col gap-3 md:w-64">
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-4 w-20" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
      <div className="hidden flex-1 md:block" />
    </div>
  )
}

function NoProfiles({ onCreate }: { onCreate: () => void }) {
  return (
    <EmptyState
      className="py-12"
      title="No profiles yet"
      description="Profiles are what goals and tasks are assigned to. Create the first one here, or with ariadne profile create."
      action={
        <Button variant="outline" size="sm" onClick={onCreate}>
          <PlusIcon />
          New profile
        </Button>
      }
    />
  )
}
