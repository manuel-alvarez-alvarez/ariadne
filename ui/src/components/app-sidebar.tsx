import { CpuIcon, TargetIcon, TerminalIcon } from "lucide-react"
import { NavLink } from "react-router-dom"

import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"

const NAV_ITEMS = [
  { to: paths.goals(), label: "Goals", icon: TargetIcon },
  { to: paths.sessions(), label: "Sessions", icon: TerminalIcon },
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
