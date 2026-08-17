/**
 * ⌘K: everything the app can show or do, one search away.
 *
 * Mounted once by the shell (`src/components/app-shell.tsx`), so it opens over
 * whatever screen is up and its picks land relative to it — a task stacks its
 * panel on the goal already open, a session opens inside its task's panel.
 *
 * It adds no requests of its own: the four lists it searches are the same
 * cache entries the goals board, the sessions screen and the profiles screen
 * read, so a palette opened after them shows their data instantly and only
 * refreshes it. They are `enabled` on open, so a session that never opens the
 * palette never fetches them either.
 *
 * With an empty query only the actions are listed. Every goal, task and
 * session of a busy orchestration is a long list to answer a question nobody
 * asked yet; the entities appear as soon as there is something to match them
 * against.
 */

import { useQuery } from "@tanstack/react-query"
import { defaultFilter, useCommandState } from "cmdk"
import {
  CpuIcon,
  ListChecksIcon,
  MoonIcon,
  PlusIcon,
  RadioTowerIcon,
  SettingsIcon,
  SunIcon,
  TargetIcon,
  TriangleAlertIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { useMemo, useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { goalsQueryOptions } from "@/features/goals/queries"
import { profilesQueryOptions } from "@/features/profiles/queries"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"
import { SETTINGS_SHORTCUT } from "@/hooks/use-global-shortcuts"
import { shortcutLabel } from "@/lib/shortcuts"
import { paths } from "@/routes/paths"

import {
  buildPaletteEntries,
  type PaletteEntries,
  type PaletteEntry,
  paletteTargetTo,
} from "./entries"
import { bestScore, preferLiteralMatches } from "./score"

/** cmdk's subsequence scoring, with the rows the query names pulled on top. */
const PALETTE_FILTER = preferLiteralMatches(defaultFilter)

export function CommandPalette({
  open,
  onOpenChange,
  onOpenSettings,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Settings is the shell's dialog: the palette asks for it rather than owning it. */
  onOpenSettings: () => void
}) {
  const navigate = useNavigate()
  const [search] = useSearchParams()
  const { resolvedTheme, setTheme } = useTheme()

  const [createGoalOpen, setCreateGoalOpen] = useState(false)

  const entries = usePaletteEntries(open)

  /** A pick always closes the palette; what it then does is the argument. */
  function run(action: () => void) {
    onOpenChange(false)
    action()
  }

  function go(entry: PaletteEntry) {
    run(() => void navigate(paletteTargetTo(entry.target, search)))
  }

  return (
    <>
      <CommandDialog
        open={open}
        onOpenChange={onOpenChange}
        className="sm:max-w-xl"
        title="Command palette"
        description="Search goals, tasks, sessions and profiles, or run an action."
      >
        {/* The search is cmdk's own state, not React's: cmdk sorts the rows by
            reordering the DOM, and a re-render on every keystroke — which is
            what holding the query up here would cause — puts them back in the
            order they were written in. The palette is unmounted while closed,
            so that state also starts empty every time. */}
        <Command loop filter={PALETTE_FILTER}>
          <CommandInput autoFocus placeholder="Search goals, tasks, sessions, profiles…" />
          <CommandList>
            <CommandEmpty>No matches.</CommandEmpty>

            <CommandGroup heading="Actions">
              <CommandItem
                value="New goal"
                keywords={["create", "add"]}
                onSelect={() => run(() => setCreateGoalOpen(true))}
              >
                <PlusIcon />
                New goal
              </CommandItem>
              <CommandItem
                value="Open settings"
                keywords={["daemon", "url", "preferences"]}
                onSelect={() => run(onOpenSettings)}
              >
                <SettingsIcon />
                Open settings
                <CommandShortcut>{shortcutLabel(SETTINGS_SHORTCUT)}</CommandShortcut>
              </CommandItem>
              <CommandItem
                value="Toggle theme"
                keywords={["dark", "light", "appearance"]}
                onSelect={() => run(() => setTheme(resolvedTheme === "dark" ? "light" : "dark"))}
              >
                {resolvedTheme === "dark" ? <SunIcon /> : <MoonIcon />}
                Toggle theme
              </CommandItem>
              {PAGES.map((page) => (
                <CommandItem
                  key={page.path}
                  value={`Go to ${page.label}`}
                  onSelect={() => run(() => void navigate(page.path))}
                >
                  <page.icon />
                  Go to {page.label}
                </CommandItem>
              ))}
            </CommandGroup>

            <PaletteEntities entries={entries} onPick={go} />
          </CommandList>
        </Command>
      </CommandDialog>

      {/* The palette's own copy: "New goal" has to work on screens that have
          no create button of their own. */}
      <CreateGoalDialog
        open={createGoalOpen}
        onOpenChange={setCreateGoalOpen}
        onCreated={(goal) => void navigate(paths.goal(goal.id))}
      />
    </>
  )
}

/** The screens the palette can jump to, in the sidebar's order. */
const PAGES = [
  { label: "Goals", path: paths.goals(), icon: TargetIcon },
  { label: "Attention", path: paths.attention(), icon: TriangleAlertIcon },
  { label: "Sessions", path: paths.sessions(), icon: RadioTowerIcon },
  { label: "Profiles", path: paths.profiles(), icon: CpuIcon },
] as const

/** The entity groups, in the order they are listed when nothing separates them. */
const GROUPS = [
  { key: "goals", heading: "Goals", icon: TargetIcon },
  { key: "tasks", heading: "Tasks", icon: ListChecksIcon },
  { key: "sessions", heading: "Sessions", icon: RadioTowerIcon },
  { key: "profiles", heading: "Profiles", icon: CpuIcon },
] as const satisfies readonly {
  key: keyof PaletteEntries
  heading: string
  icon: typeof TargetIcon
}[]

/**
 * The entity half of the list, which only exists once something has been typed.
 *
 * It subscribes to cmdk's search itself rather than being handed it, so that a
 * keystroke re-renders these rows and nothing above them — the input keeps its
 * focus and its cursor, and the palette's own layout is not rebuilt under
 * cmdk's ordering.
 *
 * The groups are then ordered by their best match, because cmdk only sorts the
 * rows within a group: without this, one strong match would sit under a group
 * of weak ones, and the row Enter takes would be the wrong one.
 */
function PaletteEntities({
  entries,
  onPick,
}: {
  entries: PaletteEntries
  onPick: (entry: PaletteEntry) => void
}) {
  const query = useCommandState((state) => state.search).trim()

  // Stable sort: groups that match equally well keep the order declared above.
  const ordered = useMemo(
    () =>
      GROUPS.map((group) => ({
        ...group,
        rows: entries[group.key],
        best: query ? bestScore(PALETTE_FILTER, entries[group.key], query) : 0,
      })).sort((a, b) => b.best - a.best),
    [entries, query],
  )

  if (!query) return null
  return (
    <>
      {ordered.map((group) => (
        <EntryGroup
          key={group.key}
          heading={group.heading}
          icon={group.icon}
          entries={group.rows}
          onPick={onPick}
        />
      ))}
    </>
  )
}

/**
 * One group of rows. Always rendered, empty or not: cmdk hides a group with no
 * matches on its own, and a group that comes and goes is a group React has to
 * insert again — which is what would undo the ordering cmdk just applied.
 */
function EntryGroup({
  heading,
  icon: Icon,
  entries,
  onPick,
}: {
  heading: string
  icon: typeof TargetIcon
  entries: PaletteEntry[]
  onPick: (entry: PaletteEntry) => void
}) {
  return (
    <CommandGroup heading={heading}>
      {entries.map((entry) => (
        <CommandItem
          key={entry.value}
          value={entry.value}
          keywords={entry.keywords}
          onSelect={() => onPick(entry)}
        >
          <Icon className="text-muted-foreground" />
          <span className="truncate">{entry.label}</span>
          {entry.detail ? (
            <span className="ml-auto shrink-0 truncate pl-2 font-mono text-xs text-muted-foreground">
              {entry.detail}
            </span>
          ) : null}
        </CommandItem>
      ))}
    </CommandGroup>
  )
}

/**
 * The four lists, read through the same query options their own screens use so
 * the palette shares their cache entries rather than making four of its own.
 */
function usePaletteEntries(open: boolean) {
  const goals = useQuery({ ...goalsQueryOptions(), enabled: open })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: open })
  const sessions = useQuery({ ...sessionsQueryOptions(), enabled: open })
  const profiles = useQuery({ ...profilesQueryOptions(), enabled: open })

  return useMemo(
    () =>
      buildPaletteEntries({
        goals: goals.data,
        tasks: tasks.data,
        sessions: sessions.data,
        profiles: profiles.data,
      }),
    [goals.data, tasks.data, sessions.data, profiles.data],
  )
}
