/**
 * The frame every screen renders inside: sidebar navigation on the left, a
 * header naming the screen and carrying the theme and settings controls, the
 * connection banner under it when the daemon is not answering, and the routed
 * screen in the middle.
 *
 * The header's title comes from the route's own `handle` (see
 * `src/routes/page-title.ts`), so this file knows nothing about which screens
 * exist.
 *
 * Shared file — feature tasks should not need to touch it. Add navigation
 * entries in `app-sidebar.tsx`, routes in your own feature's `routes.tsx`.
 */

import { SettingsIcon } from "lucide-react"
import { useState } from "react"
import { Outlet } from "react-router-dom"

import { AppSidebar } from "@/components/app-sidebar"
import { ConnectionBanner } from "@/components/connection-banner"
import { ConnectionStatus } from "@/components/connection-status"
import { DetailPanels } from "@/components/detail-panels"
import { SettingsDialog } from "@/components/settings-dialog"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { usePageTitle } from "@/routes/page-title"

export function AppShell() {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const pageTitle = usePageTitle()

  return (
    <div className="flex h-svh w-full overflow-hidden bg-background text-foreground">
      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
        <div className="flex h-12 items-center gap-2 px-4">
          <span className="font-heading text-sm font-semibold tracking-tight">Ariadne</span>
        </div>
        <Separator />
        <AppSidebar />
        <div className="mt-auto p-2">
          <ConnectionStatus className="w-full" />
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-1 border-b px-3">
          {/* The screen's own `h1` leads its content; this is where the user
           *is*, so it reads as chrome rather than as a second heading. */}
          <span className="truncate text-sm font-medium">{pageTitle}</span>
          <div className="ml-auto flex items-center gap-1">
            <ThemeToggle />
            <Button
              variant="ghost"
              size="icon"
              aria-label="Settings"
              onClick={() => setSettingsOpen(true)}
            >
              <SettingsIcon />
            </Button>
          </div>
        </header>
        <ConnectionBanner onOpenSettings={() => setSettingsOpen(true)} />
        <main className="min-h-0 flex-1 overflow-auto p-6">
          <Outlet />
        </main>
      </div>

      <DetailPanels />
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  )
}
