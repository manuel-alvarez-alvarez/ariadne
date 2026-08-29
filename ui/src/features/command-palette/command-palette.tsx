/**
 * ⌘K: everything the app can show or do, one search away.
 *
 * Mounted once by the shell (`src/components/app-shell.tsx`), so it opens over
 * whatever screen is up and its picks land relative to it — a task stacks its
 * panel on the goal already open, a session opens inside its task's panel.
 *
 * It adds no requests of its own: the four lists it searches are the same
 * cache entries the goals board, the session panels and the profiles screen
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
  BotIcon,
  CpuIcon,
  FolderGit2Icon,
  ListChecksIcon,
  MoonIcon,
  PanelLeftIcon,
  PlusIcon,
  RadioTowerIcon,
  SettingsIcon,
  SunIcon,
  TargetIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { type ReactNode, useMemo } from "react"
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { goalsQueryOptions } from "@/features/goals/queries"
import { profilesQueryOptions } from "@/features/profiles/queries"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"
import {
  NEW_GOAL_SHORTCUT,
  SETTINGS_SHORTCUT,
  SIDEBAR_SHORTCUT,
  screenShortcut,
} from "@/hooks/use-global-shortcuts"
import { keySequenceLabel, shortcutLabel } from "@/lib/shortcuts"
import { paths } from "@/routes/paths"

import { splitDetail } from "./detail"
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
  onNewGoal,
  onToggleSidebar,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Settings is the shell's dialog: the palette asks for it rather than owning it. */
  onOpenSettings: () => void
  /** So is the create-goal dialog, which `N` opens from outside the palette too. */
  onNewGoal: () => void
  /** The sidebar rail, which the shell owns and `[` toggles from outside here. */
  onToggleSidebar: () => void
}) {
  const navigate = useNavigate()
  const [search] = useSearchParams()
  const { resolvedTheme, setTheme } = useTheme()

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
              onSelect={() => run(onNewGoal)}
            >
              <PlusIcon />
              New goal
              <CommandShortcut>{keySequenceLabel(NEW_GOAL_SHORTCUT)}</CommandShortcut>
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
              value="Toggle sidebar"
              keywords={["rail", "navigation", "collapse", "expand"]}
              onSelect={() => run(onToggleSidebar)}
            >
              <PanelLeftIcon />
              Toggle sidebar
              <CommandShortcut>{keySequenceLabel(SIDEBAR_SHORTCUT)}</CommandShortcut>
            </CommandItem>
            <CommandItem
              value="Toggle theme"
              keywords={["dark", "light", "appearance"]}
              onSelect={() => run(() => setTheme(resolvedTheme === "dark" ? "light" : "dark"))}
            >
              {resolvedTheme === "dark" ? <SunIcon /> : <MoonIcon />}
              Toggle theme
            </CommandItem>
            {PAGES.map((page) => {
              // The palette is where the screen chords are written down: a
              // `G S` nobody can see is a chord nobody types.
              const chord = screenShortcut(page.path)
              return (
                <CommandItem
                  key={page.path}
                  value={`Go to ${page.label}`}
                  onSelect={() => run(() => void navigate(page.path))}
                >
                  <page.icon />
                  Go to {page.label}
                  {chord ? <CommandShortcut>{keySequenceLabel(chord)}</CommandShortcut> : null}
                </CommandItem>
              )
            })}
          </CommandGroup>

          <PaletteEntities entries={entries} onPick={go} />
        </CommandList>
        <PaletteHints />
      </Command>
    </CommandDialog>
  )
}

/**
 * What the keyboard can do in here, along the bottom — the convention every
 * palette follows, and the only place `esc` is written down (the key itself
 * belongs to Base UI; see `@/hooks/use-global-shortcuts`).
 */
function PaletteHints() {
  return (
    <div className="flex items-center gap-4 border-t px-3 py-2 text-xs text-muted-foreground">
      <Hint keys="↑↓">navigate</Hint>
      <Hint keys="↵">open</Hint>
      <Hint keys="esc">close</Hint>
    </div>
  )
}

function Hint({ keys, children }: { keys: string; children: ReactNode }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="rounded border bg-muted px-1 font-mono text-[0.7rem] leading-4">{keys}</kbd>
      {children}
    </span>
  )
}

/**
 * The screens the palette can jump to: every entry the sidebar has, with its
 * icon and its label, in its order — the palette is the sidebar for people who
 * do not reach for it (see `@/components/app-sidebar`).
 */
const PAGES = [
  { label: "Goals", path: paths.goals(), icon: TargetIcon },
  { label: "Sessions", path: paths.sessions(), icon: RadioTowerIcon },
  { label: "Profiles", path: paths.profiles(), icon: CpuIcon },
  { label: "Agents", path: paths.agents(), icon: BotIcon },
  { label: "Repositories", path: paths.repositories(), icon: FolderGit2Icon },
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
          {/* The row is named by its title, so the title is what gets the
              room: the detail is capped and truncated around it. */}
          <span className="min-w-0 flex-1 truncate">{entry.label}</span>
          {entry.detail ? <EntryDetail text={entry.detail} /> : null}
        </CommandItem>
      ))}
    </CommandGroup>
  )
}

/**
 * The row's secondary text — an id, a branch, a role — held to a third of the
 * row and truncated in the middle rather than at the end, so a branch keeps the
 * slug that tells it from the next one (see `./detail`).
 */
function EntryDetail({ text }: { text: string }) {
  const { head, tail } = splitDetail(text)
  return (
    // A tooltip, like every other hint in the app — but the one that takes no
    // focus: the palette keeps the caret in its input and every row is reached
    // with the arrow keys, so a tab stop per row would be a way *out* of the
    // search. Nothing is lost by it, since the whole detail is in the row's own
    // text either way and only the middle of it is truncated away visually.
    <Tooltip>
      <TooltipTrigger
        tabIndex={-1}
        render={
          <span className="flex max-w-[33%] min-w-0 justify-end pl-2 font-mono text-xs text-muted-foreground" />
        }
      >
        <span className="truncate">{head}</span>
        {tail ? <span className="shrink-0">{tail}</span> : null}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{text}</TooltipContent>
    </Tooltip>
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
