/**
 * The Forge screen: what the GitHub or GitLab integration of an enabled
 * repository reads, in two tabs — the pull requests that ask for the user's
 * review (029) and the open issues a goal can start from (028).
 *
 * One sidebar entry at the level of Goals or Repositories, rather than two:
 * neither list is Ariadne's own work, and both come from the same forge. Each
 * tab is a route of its own (`#/forge/pull-requests`, `#/forge/issues`), so a
 * link and a reload land on the tab, and the panels and filters a tab keeps
 * in its search params stay its own when the other is picked.
 */

import { Outlet, useLocation, useNavigate } from "react-router-dom"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { paths } from "@/routes/paths"

const TABS = [
  { value: "pull-requests", label: "Pull requests", to: paths.pullRequests() },
  { value: "issues", label: "Issues", to: paths.issues() },
] as const

export function ForgePage() {
  const { pathname } = useLocation()
  const navigate = useNavigate()
  const tab = TABS.find((each) => pathname.startsWith(each.to)) ?? TABS[0]

  return (
    <div className="flex flex-col gap-4">
      <Tabs
        value={tab.value}
        onValueChange={(value) => {
          const next = TABS.find((each) => each.value === value)
          if (next) navigate(next.to)
        }}
      >
        <TabsList>
          {TABS.map((each) => (
            <TabsTrigger key={each.value} value={each.value}>
              {each.label}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>
      <Outlet />
    </div>
  )
}
