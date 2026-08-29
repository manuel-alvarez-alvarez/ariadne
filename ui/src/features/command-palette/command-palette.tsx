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
 * With an empty query only what is stuck and the actions are listed. Every
 * goal, task and session of a busy orchestration is a long list to answer a
 * question nobody asked yet; the entities appear as soon as there is something
 * to match them against.
 *
 * Some of the actions depend on what is underneath — a task can only be created
 * in the goal whose panel is open, an attach command only copied for the task
 * or session that is — so the palette reads the screen's own search params,
 * the same way its picks are built against them.
 */

import { useQuery } from "@tanstack/react-query"
import { defaultFilter, useCommandState } from "cmdk"
import {
  BotIcon,
  CopyIcon,
  CpuIcon,
  FolderGit2Icon,
  KeyboardIcon,
  ListChecksIcon,
  ListPlusIcon,
  MoonIcon,
  PanelLeftIcon,
  PlusIcon,
  RadioTowerIcon,
  RefreshCwIcon,
  ScrollTextIcon,
  SettingsIcon,
  SunIcon,
  TargetIcon,
  TriangleAlertIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { type ReactNode, useMemo, useState } from "react"
import { useLocation, useNavigate, useSearchParams } from "react-router-dom"

import type { GoalDto } from "@/api"
import { copyEntry } from "@/components/copyable-id"
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
import { useAttention } from "@/features/goals/attention"
import { goalsQueryOptions } from "@/features/goals/queries"
import { isTerminalGoalStatus } from "@/features/goals/status"
import { profilesQueryOptions } from "@/features/profiles/queries"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"
import { CreateTaskDialog } from "@/features/tasks/task-form-dialog"
import { useConnection } from "@/hooks/use-connection"
import {
  HELP_SHORTCUT,
  NEW_GOAL_SHORTCUT,
  SETTINGS_SHORTCUT,
  SIDEBAR_SHORTCUT,
  screenShortcut,
} from "@/hooks/use-global-shortcuts"
import { attachCommand } from "@/lib/clipboard"
import { shortId } from "@/lib/format"
import { keySequenceLabel, shortcutLabel } from "@/lib/shortcuts"
import { paths, taskPanelFrom } from "@/routes/paths"

import { splitDetail } from "./detail"
import {
  attentionEntries,
  buildPaletteEntries,
  type PaletteEntries,
  type PaletteEntry,
  paletteTargetTo,
} from "./entries"
import { bestScore, preferLiteralMatches } from "./score"

/** cmdk's subsequence scoring, with the rows the query names pulled on top. */
const PALETTE_FILTER = preferLiteralMatches(defaultFilter)

/**
 * The one copy the palette offers, named the way the copy menus name it (see
 * `@/lib/clipboard`) — the row, the toast and the menu entry are the same
 * action, so they read the same.
 */
const ATTACH = { label: "Copy attach command" }

export function CommandPalette({
  open,
  onOpenChange,
  onOpenSettings,
  onNewGoal,
  onOpenLogs,
  onOpenShortcuts,
  onToggleSidebar,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Settings is the shell's dialog: the palette asks for it rather than owning it. */
  onOpenSettings: () => void
  /** So is the create-goal dialog, which `N` opens from outside the palette too. */
  onNewGoal: () => void
  /** And the daemon-logs drawer, which the footer's status button also opens. */
  onOpenLogs: () => void
  /** And the cheat sheet, which `?` opens from outside the palette. */
  onOpenShortcuts: () => void
  /** The sidebar rail, which the shell owns and `[` toggles from outside here. */
  onToggleSidebar: () => void
}) {
  const navigate = useNavigate()
  const [search] = useSearchParams()
  // Where the palette was opened decides where a pick lands, not only which
  // params it keeps: see `paletteTargetTo`.
  const { pathname } = useLocation()
  const { resolvedTheme, setTheme } = useTheme()
  const { status, retry } = useConnection()

  const { entries, goals } = usePaletteEntries(open)
  // What the screen underneath is showing, which is what the actions below act
  // on: the goal a task would be created in, and the task or session an attach
  // command would be for.
  const openGoal = goals?.find((goal) => goal.id === search.get("goal"))
  const attachTo = search.get("session") ?? search.get("task")
  /**
   * The goal the create-task dialog is open for. Held here rather than by the
   * shell, which knows nothing about which goal is on screen — and held as the
   * goal itself, since the palette's own lists stop being read the moment it
   * closes.
   */
  const [newTaskGoal, setNewTaskGoal] = useState<GoalDto | null>(null)

  /** A pick always closes the palette; what it then does is the argument. */
  function run(action: () => void) {
    onOpenChange(false)
    action()
  }

  function go(entry: PaletteEntry) {
    run(() => void navigate(paletteTargetTo(entry.target, search, pathname)))
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

            <PaletteAttention onPick={go} />

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
              {/* Only where there is a goal to create it in — its panel, or
                  on the sessions screen the goal that screen is narrowed to —
                  and only while that goal can still do anything with a task,
                  which is the rule its own panel puts on the button. */}
              {openGoal && !isTerminalGoalStatus(openGoal.status) ? (
                <CommandItem
                  value="New task"
                  keywords={["create", "add", openGoal.title]}
                  onSelect={() => run(() => setNewTaskGoal(openGoal))}
                >
                  <ListPlusIcon />
                  New task
                  {/* Which goal it would be created in — a name, so it drops
                      the letter-spacing this slot gives a chord. */}
                  <CommandShortcut className="max-w-[40%] truncate tracking-normal">
                    {openGoal.title}
                  </CommandShortcut>
                </CommandItem>
              ) : null}
              {attachTo ? (
                // The id is on its way into a terminal, which is the one place
                // the app cannot take the user: `ariadne attach <id>` is what
                // gets them there, for whatever this screen has open.
                <CommandItem
                  value={ATTACH.label}
                  keywords={["ariadne", "attach", "tmux", "terminal", attachTo]}
                  onSelect={() =>
                    run(() => void copyEntry({ ...ATTACH, text: attachCommand(attachTo) }))
                  }
                >
                  <CopyIcon />
                  {ATTACH.label}
                  {/* Whose id is about to land on the clipboard, since the
                      row is the same for a task and for a session. */}
                  <CommandShortcut className="font-mono tracking-normal">
                    {shortId(attachTo)}
                  </CommandShortcut>
                </CommandItem>
              ) : null}
              <CommandItem
                value="Open daemon logs"
                keywords={["ariadned", "stderr", "diagnostics", "trace"]}
                onSelect={() => run(onOpenLogs)}
              >
                <ScrollTextIcon />
                Open daemon logs
              </CommandItem>
              {/* The banner's own Retry, for when the banner is not what the
                  user is looking at. Absent while the daemon answers: there is
                  nothing to retry. */}
              {status === "disconnected" ? (
                <CommandItem
                  value="Retry connection"
                  keywords={["reconnect", "daemon", "stream", "offline"]}
                  onSelect={() => run(retry)}
                >
                  <RefreshCwIcon />
                  Retry connection
                </CommandItem>
              ) : null}
              <CommandItem
                value="Keyboard shortcuts"
                keywords={["chords", "keys", "help", "cheat sheet"]}
                onSelect={() => run(onOpenShortcuts)}
              >
                <KeyboardIcon />
                Keyboard shortcuts
                <CommandShortcut>{keySequenceLabel(HELP_SHORTCUT)}</CommandShortcut>
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
      {/* Outside the palette, which is gone by the time this opens: the pick
          closes it and hands the dialog the goal it was made against. */}
      {newTaskGoal ? (
        <CreateTaskDialog
          goal={newTaskGoal}
          open
          onOpenChange={(dialogOpen) => dialogOpen || setNewTaskGoal(null)}
          // The same landing the goal panel's own button gives it: the new
          // task's panel, over whatever is on screen — or on the board, from
          // the one screen where that panel does not open (`taskPanelFrom`).
          onCreated={(task) => void navigate(taskPanelFrom(pathname, search, task.id))}
        />
      ) : null}
    </>
  )
}

/**
 * What is asking for a person, above everything else, before anything has been
 * typed.
 *
 * It is a child rather than part of the palette so that {@link useAttention}'s
 * three lists are only observed while the dialog is up — the palette is
 * unmounted while closed — and those three are the same keys the palette
 * already reads, so the group costs no request of its own.
 *
 * It shows only for an empty query, which is what keeps its rows from being a
 * second copy of whatever the `Tasks` and `Sessions` groups are already
 * matching: with something typed, the entity groups are the answer.
 */
function PaletteAttention({ onPick }: { onPick: (entry: PaletteEntry) => void }) {
  const query = useCommandState((state) => state.search).trim()
  const { items } = useAttention()
  const entries = useMemo(() => attentionEntries(items), [items])

  if (query || entries.length === 0) return null
  return (
    <EntryGroup
      heading="Needs attention"
      icon={TriangleAlertIcon}
      entries={entries}
      onPick={onPick}
    />
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
 *
 * The goals come back as themselves as well as as rows: "New task" needs the
 * goal the screen has open, not a row that would navigate to it.
 */
function usePaletteEntries(open: boolean): {
  entries: PaletteEntries
  goals: GoalDto[] | undefined
} {
  const goals = useQuery({ ...goalsQueryOptions(), enabled: open })
  const tasks = useQuery({ ...taskListQueryOptions(), enabled: open })
  const sessions = useQuery({ ...sessionsQueryOptions(), enabled: open })
  const profiles = useQuery({ ...profilesQueryOptions(), enabled: open })

  const entries = useMemo(
    () =>
      buildPaletteEntries({
        goals: goals.data,
        tasks: tasks.data,
        sessions: sessions.data,
        profiles: profiles.data,
      }),
    [goals.data, tasks.data, sessions.data, profiles.data],
  )

  return { entries, goals: goals.data }
}
