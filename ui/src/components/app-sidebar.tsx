/**
 * The app's navigation. Two screens, in the order an orchestrator asks for
 * them: what is being worked on, and what the agents run as.
 *
 * What is *stuck* has no entry — it is a strip on the goals board, above the
 * lanes it is about (see `features/goals/attention-strip.tsx`). Neither do the
 * agents themselves: a session is opened from the panel of the goal or the
 * task it runs, in a panel of its own (see `features/sessions/session-panel.tsx`).
 *
 * The labels are the ones the shell's header shows, which come from each
 * route's own `handle` (see `src/routes/page-title.ts`) — they are written
 * twice on purpose rather than derived from the route table, because a sidebar
 * entry and a route are not the same list: not every route belongs here.
 */

import { CpuIcon, TargetIcon } from "lucide-react"
import { NavLink } from "react-router-dom"

import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"

const NAV_ITEMS = [
  { to: paths.goals(), label: "Goals", icon: TargetIcon },
  { to: paths.profiles(), label: "Profiles", icon: CpuIcon },
] as const

export function AppSidebar() {
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
        </NavLink>
      ))}
    </nav>
  )
}
