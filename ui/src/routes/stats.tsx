/**
 * The Stats screen: a Perses dashboard, rendered by the `ui/dashboard`
 * package in a same-origin iframe (see its own `README.md` for the package
 * and the build that feeds `/dashboard/`).
 *
 * The header holds a picker over every dashboard the daemon knows
 * (`GET /v1/dashboards`), New dashboard (create empty or copy the picked
 * one), Reset (disabled while the picked one is already the shipped
 * document), and Rename and Delete — both disabled for the built-in
 * dashboard, which the daemon refuses to delete (409 `dashboard_builtin`).
 * The iframe fills the rest of the screen.
 *
 * The iframe is handed `daemon`, `dashboard` and `theme` on its own query
 * string, which is all it reads to open — see `ui/dashboard/src/page.tsx`.
 * After that, two `postMessage`s keep it in step with this window rather than
 * reopening it on every change:
 *
 * - `{type: "theme", theme}`, whenever the app's resolved theme changes;
 * - `{type: "reload"}`, whenever the picked dashboard's own `updated_at`
 *   moves under us — a save from another window, or from the CLI — landing
 *   here as a `dashboard_updated` event the SSE dispatcher already patched
 *   into this row (`src/events/dispatch.ts`). The iframe's own save does not
 *   need this: it already holds what it just wrote.
 *
 * Both are skipped on the dashboard picked changing, since changing `src`
 * already reopens the iframe on the new one — which is also why `src`'s own
 * `theme` is frozen to whatever it was when the daemon or the picked
 * dashboard last changed (`srcTheme`), rather than tracking the live theme:
 * an iframe renavigates on any change to its `src`, the same as a browser
 * tab following a typed address, so a live value there would reopen the
 * iframe on every theme toggle and throw away whatever was open in Perses.
 */

import { useQuery } from "@tanstack/react-query"
import { PencilIcon, PlusIcon, Trash2Icon, Undo2Icon } from "lucide-react"
import { useTheme } from "next-themes"
import { useEffect, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { NewDashboardDialog } from "@/features/dashboard/new-dashboard-dialog"
import {
  dashboardsQueryOptions,
  useDeleteDashboard,
  useResetDashboard,
} from "@/features/dashboard/queries"
import { RenameDashboardDialog } from "@/features/dashboard/rename-dashboard-dialog"
import { cn } from "@/lib/format"
import { DASHBOARD_PARAM } from "@/routes/paths"
import { useBaseUrl } from "@/stores/settings"

/** The dashboard page's own origin: where every `postMessage` here is sent. */
function iframeSrc(baseUrl: string, name: string, theme: string): string {
  const params = new URLSearchParams({ daemon: baseUrl, dashboard: name, theme })
  return `/dashboard/?${params.toString()}`
}

export function StatsPage() {
  const [search, setSearch] = useSearchParams()
  const baseUrl = useBaseUrl()
  const { resolvedTheme } = useTheme()
  const theme = resolvedTheme ?? "light"
  const dashboards = useQuery(dashboardsQueryOptions())
  const reset = useResetDashboard()
  const deleteDashboard = useDeleteDashboard()
  const [confirmReset, setConfirmReset] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [newOpen, setNewOpen] = useState(false)
  const [renameOpen, setRenameOpen] = useState(false)
  const iframeRef = useRef<HTMLIFrameElement>(null)

  const selectedName = search.get(DASHBOARD_PARAM) ?? dashboards.data?.[0]?.name
  const selected = dashboards.data?.find((dashboard) => dashboard.name === selectedName)

  // The iframe's `src` carries the theme only as its *opening* value — once
  // open, a theme change is a `postMessage`, never a fresh `src`, which would
  // navigate the iframe and throw away whatever the user has open in Perses.
  // Changing the frozen value back out is keyed to the daemon and the picked
  // dashboard, the two that *should* reopen it on a fresh `src`.
  const srcTheme = useRef({ key: "", theme })
  if (srcTheme.current.key !== `${baseUrl}|${selected?.name}`) {
    srcTheme.current = { key: `${baseUrl}|${selected?.name}`, theme }
  }

  function select(name: string) {
    const next = new URLSearchParams(search)
    next.set(DASHBOARD_PARAM, name)
    setSearch(next, { replace: true })
  }

  const postedTheme = useRef(theme)
  useEffect(() => {
    if (postedTheme.current === theme) return
    postedTheme.current = theme
    iframeRef.current?.contentWindow?.postMessage({ type: "theme", theme }, window.location.origin)
  }, [theme])

  const seen = useRef<{ name: string; updatedAt: string } | undefined>(undefined)
  useEffect(() => {
    if (!selected) return
    if (seen.current?.name !== selected.name) {
      seen.current = { name: selected.name, updatedAt: selected.updated_at }
      return
    }
    if (seen.current.updatedAt === selected.updated_at) return
    seen.current = { name: selected.name, updatedAt: selected.updated_at }
    iframeRef.current?.contentWindow?.postMessage({ type: "reload" }, window.location.origin)
  }, [selected])

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Stats"
        description="A Perses dashboard, read and edited from this window."
        actions={
          <>
            <Select value={selectedName ?? ""} onValueChange={(value) => value && select(value)}>
              <SelectTrigger aria-label="Dashboard" className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(dashboards.data ?? []).map((dashboard) => (
                  <SelectItem key={dashboard.name} value={dashboard.name}>
                    {dashboard.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button variant="outline" onClick={() => setNewOpen(true)}>
              <PlusIcon />
              New dashboard
            </Button>
            <Button
              variant="outline"
              disabled={!selected || selected.document_is_default || reset.isPending}
              onClick={() => setConfirmReset(true)}
            >
              <Undo2Icon />
              Reset
            </Button>
            <Button
              variant="outline"
              disabled={!selected || selected.builtin}
              onClick={() => setRenameOpen(true)}
            >
              <PencilIcon />
              Rename
            </Button>
            <Button
              variant="outline"
              className={cn("text-destructive")}
              disabled={!selected || selected.builtin}
              onClick={() => setConfirmDelete(true)}
            >
              <Trash2Icon />
              Delete
            </Button>
          </>
        }
      />
      {dashboards.isPending ? (
        <Skeleton className="min-h-0 flex-1" />
      ) : dashboards.isError ? (
        <ErrorState
          title="Could not load dashboards"
          error={dashboards.error}
          onRetry={() => void dashboards.refetch()}
          showIcon
        />
      ) : selected ? (
        <iframe
          ref={iframeRef}
          title={`${selected.name} dashboard`}
          src={iframeSrc(baseUrl, selected.name, srcTheme.current.theme)}
          className="min-h-0 flex-1 rounded-md border"
        />
      ) : (
        <EmptyState emphasis="quiet" className="my-auto" title="No dashboard to show." />
      )}
      <ConfirmDialog
        open={confirmReset}
        onClose={() => setConfirmReset(false)}
        title={selected ? `Reset ${selected.name}?` : "Reset?"}
        description="Throws away what was saved over this dashboard and goes back to the one Ariadne ships."
        confirmLabel="Reset"
        pending={reset.isPending}
        error={reset.error}
        onConfirm={() =>
          selected && reset.mutate(selected.name, { onSuccess: () => setConfirmReset(false) })
        }
      />
      <ConfirmDialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={selected ? `Delete ${selected.name}?` : "Delete?"}
        description="This is permanent. There is no undo."
        confirmLabel="Delete"
        destructive
        pending={deleteDashboard.isPending}
        error={deleteDashboard.error}
        onConfirm={() => {
          if (!selected) return
          const deletedName = selected.name
          const remaining = (dashboards.data ?? []).find((one) => one.name !== deletedName)
          deleteDashboard.mutate(deletedName, {
            onSuccess: () => {
              setConfirmDelete(false)
              if (remaining) select(remaining.name)
            },
          })
        }}
      />
      <NewDashboardDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        current={selected}
        onCreated={(dashboard) => select(dashboard.name)}
      />
      {selected ? (
        <RenameDashboardDialog
          dashboard={selected}
          open={renameOpen}
          onOpenChange={setRenameOpen}
          onRenamed={(dashboard) => select(dashboard.name)}
        />
      ) : null}
    </div>
  )
}
