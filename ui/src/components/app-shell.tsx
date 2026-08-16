/**
 * The frame every screen renders inside: sidebar navigation on the left, a
 * header carrying the daemon connection state, theme and settings, and the
 * routed screen in the middle.
 *
 * Shared file — feature tasks should not need to touch it. Add navigation
 * entries in `app-sidebar.tsx`, routes in your own feature's `routes.tsx`.
 */

import { SettingsIcon } from "lucide-react"
import { useState } from "react"
import { Outlet } from "react-router-dom"

import { AppSidebar } from "@/components/app-sidebar"
import { ConnectionStatus } from "@/components/connection-status"
import { DetailPanels } from "@/components/detail-panels"
import { SettingsDialog } from "@/components/settings-dialog"
import { ThemeToggle } from "@/components/theme-toggle"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"

export function AppShell() {
  const [settingsOpen, setSettingsOpen] = useState(false)

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
        <header className="flex h-12 shrink-0 items-center justify-end gap-1 border-b px-3">
          <ThemeToggle />
          <Button
            variant="ghost"
            size="icon"
            aria-label="Settings"
            onClick={() => setSettingsOpen(true)}
          >
            <SettingsIcon />
          </Button>
        </header>
        <main className="min-h-0 flex-1 overflow-auto p-6">
          <Outlet />
        </main>
      </div>

      <DetailPanels />
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  )
}
