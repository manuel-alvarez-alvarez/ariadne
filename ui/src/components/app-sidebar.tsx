/**
 * The app's navigation. Five screens, in the order an orchestrator asks for
 * them: what is being worked on, who is working on it right now, what the
 * agents run as, how the agent CLIs themselves are launched, and the checkouts
 * they work in.
 *
 * What is *stuck* has no entry — it is a strip on the goals board, above the
 * lanes it is about (see `features/goals/attention-strip.tsx`). "Sessions" is
 * the agents that are running, across every goal; "Agents" is the CLIs and
 * their flags, not what is running on them. A single session still has no entry
 * of its own: it opens as a panel over whichever list picked it (see
 * `features/sessions/session-panel.tsx`).
 *
 * The labels are the ones the shell's header shows, which come from each
 * route's own `handle` (see `src/routes/router.tsx`) — they are written
 * twice on purpose rather than derived from the route table, because a sidebar
 * entry and a route are not the same list: not every route belongs here.
 */

import { BotIcon, CpuIcon, FolderGit2Icon, RadioTowerIcon, TargetIcon } from "lucide-react"
import { NavLink } from "react-router-dom"

import { cn } from "@/lib/format"
import { paths } from "@/routes/paths"

const NAV_ITEMS = [
  { to: paths.goals(), label: "Goals", icon: TargetIcon },
  { to: paths.sessions(), label: "Sessions", icon: RadioTowerIcon },
  { to: paths.profiles(), label: "Profiles", icon: CpuIcon },
  { to: paths.agents(), label: "Agents", icon: BotIcon },
  { to: paths.repositories(), label: "Repositories", icon: FolderGit2Icon },
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
