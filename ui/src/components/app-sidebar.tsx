/**
 * The app's navigation. Four screens, in the order an orchestrator asks for
 * them: what is being worked on, what is stuck, who is running it, and what
 * they run as.
 *
 * The labels are the ones the shell's header shows, which come from each
 * route's own `handle` (see `src/routes/page-title.ts`) — they are written
 * twice on purpose rather than derived from the route table, because a sidebar
 * entry and a route are not the same list: not every route belongs here.
 */

import { CpuIcon, RadioTowerIcon, TargetIcon, TriangleAlertIcon } from "lucide-react"
import { NavLink } from "react-router-dom"

import { useAttention } from "@/features/attention/queries"
import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"

const NAV_ITEMS = [
  { to: paths.goals(), label: "Goals", icon: TargetIcon },
  { to: paths.attention(), label: "Attention", icon: TriangleAlertIcon },
  { to: paths.sessions(), label: "Sessions", icon: RadioTowerIcon },
  { to: paths.profiles(), label: "Profiles", icon: CpuIcon },
] as const

export function AppSidebar() {
  // The three lists this counts are shared cache entries the attention screen
  // reads too, and the SSE dispatcher keeps them current — so the badge is live
  // and costs one extra request for the whole app.
  const { count } = useAttention()

  return (
    <nav aria-label="Main" className="flex flex-col gap-1 p-2">
      {NAV_ITEMS.map(({ to, label, icon: Icon }) => (
        <NavLink
          key={to}
          to={to}
          className={({ isActive }) =>
            cn(
              "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm font-medium transition-colors",
              isActive
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground",
            )
          }
        >
          <Icon className="size-4 shrink-0" />
          {label}
          {to === paths.attention() && count > 0 ? (
            <span className="ml-auto rounded-full bg-status-danger-soft px-1.5 py-0.5 text-xs font-medium tabular-nums text-status-danger-fg">
              {count}
              <span className="sr-only"> needing attention</span>
            </span>
          ) : null}
        </NavLink>
      ))}
    </nav>
  )
}
