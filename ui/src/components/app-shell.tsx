/**
 * The frame every screen renders inside: sidebar navigation on the left, a
 * header naming the screen and carrying the search, theme and settings
 * controls, the connection banner under it when the daemon is not answering,
 * the routed screen in the middle, and a slim footer carrying the daemon
 * connection status — whose click opens the daemon-logs drawer, mounted here
 * for the same reason the dialogs are.
 *
 * The header's title comes from the route's own `handle` (see
 * `src/routes/router.tsx`), so this file knows nothing about which screens
 * exist.
 *
 * It also binds the app's global chords and mounts what they open — the command
 * palette, the settings dialog and the create-goal dialog — because all three
 * have to work from every screen, over any panel.
 *
 * Shared file — feature tasks should not need to touch it. Add navigation
 * entries in `app-sidebar.tsx`, routes in your own feature's `routes.tsx`.
 */

import { SearchIcon, SettingsIcon } from "lucide-react"
import { useCallback, useState } from "react"
import { Outlet, useMatches, useNavigate } from "react-router-dom"

import { AppSidebar } from "@/components/app-sidebar"
import { ConnectionBanner } from "@/components/connection-banner"
import { ConnectionStatus } from "@/components/connection-status"
import { DetailPanels } from "@/components/detail-panels"
import { SettingsDialog } from "@/components/settings-dialog"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { CommandPalette } from "@/features/command-palette/command-palette"
import { AttentionAlerts } from "@/features/goals/attention-alerts"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { DaemonLogsDrawer } from "@/features/system/daemon-logs-drawer"
import { PALETTE_SHORTCUT, useGlobalShortcuts } from "@/hooks/use-global-shortcuts"
import { shortcutLabel } from "@/lib/shortcuts"
import { paths } from "@/routes/paths"

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

export function AppShell() {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [createGoalOpen, setCreateGoalOpen] = useState(false)
  const [logsOpen, setLogsOpen] = useState(false)
  const pageTitle = usePageTitle()
  const navigate = useNavigate()

  const openPalette = useCallback(() => setPaletteOpen(true), [])
  const openSettings = useCallback(() => setSettingsOpen(true), [])
  const openCreateGoal = useCallback(() => setCreateGoalOpen(true), [])
  const goToScreen = useCallback((path: string) => void navigate(path), [navigate])
  useGlobalShortcuts({
    onOpenPalette: openPalette,
    onOpenSettings: openSettings,
    onNewGoal: openCreateGoal,
    onNavigate: goToScreen,
  })

  return (
    <div className="flex h-svh w-full flex-col overflow-hidden bg-background text-foreground">
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
          {/* `border-b` inside the same h-12 box as the header's, so the two
              lines meet at the sidebar edge instead of sitting 1px apart. */}
          <div className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
            <span className="font-heading text-sm font-semibold tracking-tight">
              Ariadne Desktop
            </span>
          </div>
          <AppSidebar />
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-12 shrink-0 items-center gap-1 border-b px-3">
            {/* The screen's own `h1` leads its content; this is where the user
             *is*, so it reads as chrome rather than as a second heading. */}
            <span className="truncate text-sm font-medium">{pageTitle}</span>
            <div className="ml-auto flex items-center gap-1">
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
          <main className="min-h-0 flex-1 overflow-auto p-6">
            <Outlet />
          </main>
        </div>
      </div>

      {/* Below the sidebar row, so the status bar runs the full window width. */}
      <footer className="flex h-7 shrink-0 items-center border-t bg-muted/40 px-1">
        <ConnectionStatus onOpenLogs={() => setLogsOpen(true)} />
      </footer>

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
      />
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      <DaemonLogsDrawer open={logsOpen} onOpenChange={setLogsOpen} />
      {/* The shell's, like settings: "New goal" has to work from the palette,
          from `N`, and on screens with no create button of their own. */}
      <CreateGoalDialog
        open={createGoalOpen}
        onOpenChange={setCreateGoalOpen}
        onCreated={(goal) => void navigate(paths.goal(goal.id))}
      />
    </div>
  )
}
