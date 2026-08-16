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
import { useEffect, useMemo, useState } from "react"
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

import { buildPaletteEntries, type PaletteEntry, paletteTargetTo } from "./entries"

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

  const [query, setQuery] = useState("")
  const [createGoalOpen, setCreateGoalOpen] = useState(false)

  const entries = usePaletteEntries(open)

  // Every open starts from an empty search, never from the last one.
  useEffect(() => {
    if (open) setQuery("")
  }, [open])

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
        <Command loop>
          <CommandInput
            autoFocus
            value={query}
            onValueChange={setQuery}
            placeholder="Search goals, tasks, sessions, profiles…"
          />
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

            {/* Entities only once there is something to filter them by. */}
            {query ? (
              <>
                <EntryGroup heading="Goals" icon={TargetIcon} entries={entries.goals} onPick={go} />
                <EntryGroup
                  heading="Tasks"
                  icon={ListChecksIcon}
                  entries={entries.tasks}
                  onPick={go}
                />
                <EntryGroup
                  heading="Sessions"
                  icon={RadioTowerIcon}
                  entries={entries.sessions}
                  onPick={go}
                />
                <EntryGroup
                  heading="Profiles"
                  icon={CpuIcon}
                  entries={entries.profiles}
                  onPick={go}
                />
              </>
            ) : null}
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
  if (entries.length === 0) return null
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
