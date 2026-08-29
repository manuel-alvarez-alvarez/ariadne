/**
 * The app's navigation. Five screens, in the order an orchestrator asks for
 * them: what is being worked on, who is working on it right now, what the
 * agents run as, how the agent CLIs themselves are launched, and the checkouts
 * they work in.
 *
 * What is *stuck* has no entry of its own — it is a strip on the goals board,
 * above the lanes it is about (see `features/goals/attention-strip.tsx`), and a
 * count on the Goals entry here, which is what says an agent is waiting while
 * the user is on one of the other four screens. "Sessions" is
 * the agents that are running, across every goal; "Agents" is the CLIs and
 * their flags, not what is running on them. A single session still has no entry
 * of its own: it opens as a panel over whichever list picked it (see
 * `features/sessions/session-panel.tsx`).
 *
 * The labels are the ones the shell's header shows, which come from each
 * route's own `handle` (see `src/routes/router.tsx`) — they are written
 * twice on purpose rather than derived from the route table, because a sidebar
 * entry and a route are not the same list: not every route belongs here.
 *
 * Folded down to a rail ({@link AppSidebar.collapsed}) it is the same list with
 * the labels taken off the screen but not out of the accessibility tree: each
 * entry keeps its name as an `aria-label`, and a pointer gets it back as a
 * tooltip on the icon. The tooltip is inside the link and takes no tab stop of
 * its own — the link is already the stop, and its name already says this.
 */

import {
  BotIcon,
  CpuIcon,
  FolderGit2Icon,
  type LucideIcon,
  RadioTowerIcon,
  TargetIcon,
} from "lucide-react"
import { NavLink } from "react-router-dom"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { AttentionBadge } from "@/features/goals/attention-alerts"
import { cn } from "@/lib/format"
import { paths } from "@/routes/paths"

/**
 * `counts` is what needs attention, on the entry the strip that lists it lives
 * under: a badge here is the only thing that says an agent is waiting on the
 * user while they are on another screen (see `attention-alerts.tsx`).
 */
const NAV_ITEMS: { to: string; label: string; icon: LucideIcon; counts?: boolean }[] = [
  { to: paths.goals(), label: "Goals", icon: TargetIcon, counts: true },
  { to: paths.sessions(), label: "Sessions", icon: RadioTowerIcon },
  { to: paths.profiles(), label: "Profiles", icon: CpuIcon },
  { to: paths.agents(), label: "Agents", icon: BotIcon },
  { to: paths.repositories(), label: "Repositories", icon: FolderGit2Icon },
]

export function AppSidebar({ collapsed = false }: { collapsed?: boolean }) {
  return (
    <nav aria-label="Main" className="flex flex-col gap-1 p-2">
      {NAV_ITEMS.map(({ to, label, icon: Icon, counts }) => (
        <NavLink
          key={to}
          to={to}
          aria-label={label}
          className={({ isActive }) =>
            cn(
              "flex items-center gap-2 rounded-md py-1.5 text-sm font-medium transition-colors",
              collapsed ? "justify-center px-0" : "px-2",
              isActive
                ? "bg-sidebar-accent text-sidebar-accent-foreground"
                : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground",
            )
          }
        >
          {collapsed ? (
            // The count comes with the icon rather than after it: the row is
            // centred and sized to its content here, so the badge's own
            // `ml-auto` has no free space to push into and the two read as one
            // mark. It stays inside the trigger so hovering either half names
            // the screen.
            <Tooltip>
              <TooltipTrigger tabIndex={-1} render={<span className="flex items-center gap-1" />}>
                <Icon className="size-4 shrink-0" />
                {counts ? <AttentionBadge /> : null}
              </TooltipTrigger>
              <TooltipContent side="right">{label}</TooltipContent>
            </Tooltip>
          ) : (
            <>
              <Icon className="size-4 shrink-0" />
              {label}
              {counts ? <AttentionBadge /> : null}
            </>
          )}
        </NavLink>
      ))}
    </nav>
  )
}
