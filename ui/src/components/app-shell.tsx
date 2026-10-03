/**
 * The frame every screen renders inside: sidebar navigation on the left,
 * ending in the daemon connection status, and a main area under one header
 * bar — the screen's name as its `h1`, and the search, theme and settings
 * controls pinned at the header's far end.
 *
 * The header's title comes from the route's own `handle` (see
 * `src/routes/router.tsx`), so this file knows nothing about which screens
 * exist. A screen's own actions and the one-line description that used to sit
 * under a second heading now reach this header through `PageHeaderContext`:
 * `PageHeader` (`page-header.tsx`) portals its `actions` into the slot this
 * file renders at the header's end, and hands its `description` to
 * `setDescription`, which becomes the title's tooltip. That keeps `PageHeader`
 * itself free of layout — it registers content, this file places it — so a
 * feature page's call site never has to know whether it is framed by the real
 * shell or mounted on its own in a test.
 *
 * It also binds the app's global chords and mounts what they open — the command
 * palette, the settings dialog, the create-goal dialog and the keyboard cheat
 * sheet — because all of them have to work from every screen, over any panel.
 *
 * The sidebar folds down to an icon rail, from the header's own button or the
 * `[` chord. That is what makes the goals board fit a 1280px laptop without
 * scrolling sideways: 12rem of navigation is 12rem the five pipeline columns
 * do not get. Which way it is left is persisted, so it survives a restart.
 *
 * Shared file — feature tasks should not need to touch it. Add navigation
 * entries in `app-sidebar.tsx`, routes in your own feature's `routes.tsx`.
 */

import { PanelLeftCloseIcon, PanelLeftOpenIcon, SearchIcon, SettingsIcon } from "lucide-react"
import { createContext, type ReactNode, useCallback, useMemo, useState } from "react"
import { Outlet, useMatches, useNavigate } from "react-router-dom"

import { AppSidebar } from "@/components/app-sidebar"
import { ConnectionBanner } from "@/components/connection-banner"
import { ConnectionStatus } from "@/components/connection-status"
import { DetailPanels } from "@/components/detail-panels"
import { KeyboardShortcutsDialog } from "@/components/keyboard-shortcuts-dialog"
import { SettingsDialog } from "@/components/settings-dialog"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { CommandPalette } from "@/features/command-palette/command-palette"
import { AttentionAlerts } from "@/features/goals/attention-alerts"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { DaemonLogsDrawer } from "@/features/system/daemon-logs-drawer"
import { PALETTE_SHORTCUT, useGlobalShortcuts } from "@/hooks/use-global-shortcuts"
import { cn } from "@/lib/format"
import { shortcutLabel } from "@/lib/shortcuts"
import { paths } from "@/routes/paths"
import { useSettingsStore } from "@/stores/settings"

/** What a route declares so the header can name the screen it is framing. */
export interface PageHandle {
  /** What the header calls this screen; matches its sidebar entry. */
  title: string
}

/**
 * The title of the deepest matched route that declares one, or `null` for the
 * screens that do not (the redirects, the not-found page).
 */
function usePageTitle(): string | null {
  const matches = useMatches()
  for (let i = matches.length - 1; i >= 0; i -= 1) {
    const handle = matches[i]?.handle as Partial<PageHandle> | undefined
    if (handle?.title) return handle.title
  }
  return null
}

/**
 * What a mounted `PageHeader` reaches through to put itself in the header:
 * `actionsSlot` is the DOM node its `actions` portal into — the header's end,
 * before search, theme and settings — and `setDescription` hands the header
 * title's tooltip its one line. `null` outside the shell, so a `PageHeader`
 * rendered on its own (every feature test) falls back to rendering its
 * `actions` inline instead of reaching for a slot that does not exist.
 */
interface PageHeaderSlot {
  actionsSlot: HTMLDivElement | null
  setDescription: (description: ReactNode) => void
}

export const PageHeaderContext = createContext<PageHeaderSlot | null>(null)

/** A thread wound on a spool: the app's mark. Always 16px, always `text-primary`. */
function AriadneMark() {
  return (
    <svg
      viewBox="0 0 16 16"
      className="size-4 shrink-0 text-primary"
      aria-hidden="true"
      fill="none"
    >
      <path
        d="M4 3c0 1.66 1.79 3 4 3s4-1.34 4-3M4 13c0-1.66 1.79-3 4-3s4 1.34 4 3M4 3v10M12 3v10"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path
        d="M4.6 5.4 11.4 10.6"
        stroke="currentColor"
        strokeWidth="1"
        strokeLinecap="round"
        opacity="0.6"
      />
    </svg>
  )
}

export function AppShell() {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [createGoalOpen, setCreateGoalOpen] = useState(false)
  const [logsOpen, setLogsOpen] = useState(false)
  const [shortcutsOpen, setShortcutsOpen] = useState(false)
  const [actionsSlot, setActionsSlot] = useState<HTMLDivElement | null>(null)
  const [description, setDescription] = useState<ReactNode>(null)
  const pageTitle = usePageTitle()
  const navigate = useNavigate()
  const railed = useSettingsStore((state) => state.sidebarCollapsed)
  const toggleSidebar = useSettingsStore((state) => state.toggleSidebar)

  const openPalette = useCallback(() => setPaletteOpen(true), [])
  const openSettings = useCallback(() => setSettingsOpen(true), [])
  const openCreateGoal = useCallback(() => setCreateGoalOpen(true), [])
  const openLogs = useCallback(() => setLogsOpen(true), [])
  const openShortcuts = useCallback(() => setShortcutsOpen(true), [])
  const goToScreen = useCallback((path: string) => void navigate(path), [navigate])
  useGlobalShortcuts({
    onOpenPalette: openPalette,
    onOpenSettings: openSettings,
    onNewGoal: openCreateGoal,
    onOpenShortcuts: openShortcuts,
    onNavigate: goToScreen,
    onToggleSidebar: toggleSidebar,
  })

  const pageHeaderSlot = useMemo<PageHeaderSlot>(
    () => ({ actionsSlot, setDescription }),
    [actionsSlot],
  )

  return (
    <PageHeaderContext.Provider value={pageHeaderSlot}>
      <div className="flex h-svh w-full flex-col overflow-hidden bg-background text-foreground">
        <div className="flex min-h-0 flex-1">
          <aside
            className={cn(
              "flex shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground",
              railed ? "w-14" : "w-48",
            )}
          >
            {/* `border-b` inside the same h-12 box as the header's, so the two
                lines meet at the sidebar edge instead of sitting 1px apart. */}
            <div
              className={cn(
                "flex h-12 shrink-0 items-center gap-2 border-b",
                railed ? "justify-center px-0" : "px-4",
              )}
            >
              {/* The rail keeps the mark and drops the word: the box is 3.5rem
                  wide, and a truncated product name says less than its mark.
                  Named by a tooltip there, where the word beside it is gone. */}
              {railed ? (
                <Tooltip>
                  <TooltipTrigger tabIndex={-1} render={<span className="flex" />}>
                    <AriadneMark />
                  </TooltipTrigger>
                  <TooltipContent side="right">Ariadne</TooltipContent>
                </Tooltip>
              ) : (
                <>
                  <AriadneMark />
                  <span className="font-heading text-sm font-semibold tracking-tight">Ariadne</span>
                </>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto">
              <AppSidebar collapsed={railed} />
            </div>
            {/* The sidebar's own last child, where the footer used to carry
                the same readout: one bar, not two, saying the same thing. No
                wrapper around it: the status button is that last child. */}
            <ConnectionStatus
              collapsed={railed}
              onOpenLogs={openLogs}
              className={cn("w-full rounded-none border-t", railed ? "p-2" : "px-3 py-2")}
            />
          </aside>

          <div className="flex min-w-0 flex-1 flex-col">
            <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
              {/* Beside the edge it moves, and pressed while the rail is down —
                  a toggle, not a command, and the one control that says which
                  way the sidebar is. */}
              <Button
                variant="ghost"
                size="icon"
                aria-label={railed ? "Expand sidebar" : "Collapse sidebar"}
                aria-pressed={railed}
                onClick={toggleSidebar}
              >
                {railed ? <PanelLeftOpenIcon /> : <PanelLeftCloseIcon />}
              </Button>
              {/* The screen's one heading, at the app's one title size. A
                  routed screen's own `PageHeader` hands this its description
                  through `PageHeaderContext`, shown here as the tooltip. */}
              {description ? (
                <Tooltip>
                  {/* biome-ignore lint/a11y/useHeadingContent: Base UI clones this element and
                      fills it with the children passed to `TooltipTrigger` below, `pageTitle` */}
                  <TooltipTrigger render={<h1 className="truncate text-base font-semibold" />}>
                    {pageTitle}
                  </TooltipTrigger>
                  <TooltipContent side="bottom" align="start">
                    {description}
                  </TooltipContent>
                </Tooltip>
              ) : (
                <h1 className="truncate text-base font-semibold">{pageTitle}</h1>
              )}
              {/* Where a `PageHeader`'s `actions` portal in — the header's
                  own end, pushed there by this `ml-auto` so they land right
                  before search, theme and settings rather than beside the
                  title. Empty on a screen that passes none. */}
              <div ref={setActionsSlot} className="ml-auto flex items-center gap-2" />
              <div className="flex items-center gap-1">
                {/* The palette's affordance: a chord nobody can see is a chord
                    nobody uses, so the header carries it with its hint. */}
                <Button
                  variant="outline"
                  size="sm"
                  className="gap-2 text-muted-foreground font-normal"
                  onClick={openPalette}
                >
                  <SearchIcon />
                  Search
                  <kbd className="rounded border bg-muted px-1 font-mono text-[0.7rem] leading-4">
                    {shortcutLabel(PALETTE_SHORTCUT)}
                  </kbd>
                </Button>
                <ThemeToggle />
                <Button variant="ghost" size="icon" aria-label="Settings" onClick={openSettings}>
                  <SettingsIcon />
                </Button>
              </div>
            </header>
            <ConnectionBanner onOpenSettings={openSettings} />
            <main className="min-h-0 flex-1 overflow-auto p-4">
              <Outlet />
            </main>
          </div>
        </div>

        {/* Draws nothing: the window's title, the sidebar's count and the toast
            for an agent that got stuck while the user was on another screen. It
            is mounted here for the same reason the dialogs are — it has to be
            true of every screen, not of the board alone. */}
        <AttentionAlerts />
        <DetailPanels />
        <CommandPalette
          open={paletteOpen}
          onOpenChange={setPaletteOpen}
          onOpenSettings={openSettings}
          onNewGoal={openCreateGoal}
          onOpenLogs={openLogs}
          onOpenShortcuts={openShortcuts}
          onToggleSidebar={toggleSidebar}
        />
        <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
        <DaemonLogsDrawer open={logsOpen} onOpenChange={setLogsOpen} />
        {/* `?` from anywhere, and a palette row for the people who reach for the
            palette first; the sheet is where every other chord is written down. */}
        <KeyboardShortcutsDialog open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
        {/* The shell's, like settings: "New goal" has to work from the palette,
            from `N`, and on screens with no create button of their own. */}
        <CreateGoalDialog
          open={createGoalOpen}
          onOpenChange={setCreateGoalOpen}
          onCreated={(goal) => void navigate(paths.goal(goal.id))}
        />
      </div>
    </PageHeaderContext.Provider>
  )
}
