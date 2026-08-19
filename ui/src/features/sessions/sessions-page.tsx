/**
 * The sessions screen: every agent Ariadne has run, whatever it was run for.
 *
 * The panels answer "what is this task's engineer doing"; this screen answers
 * the question no panel can — "what is running right now", and "which agent
 * failed while I was away". It is the one place {@link SessionsList} is mounted
 * unscoped, which is what turns its Context column on: with no goal and no task
 * around the table, the row has to say which piece of work it belongs to.
 *
 * A row opens the session's own panel (`?session=`) over this screen rather
 * than navigating anywhere, so the list — and the filters that produced it —
 * stays behind it with the picked row still marked.
 *
 * The two filters live in the URL, next to `?session=`, and are remembered
 * between visits: see `filters.ts` and `use-session-filters.ts`, which are the
 * goals board's pair of the same name for two single-select params.
 */

import { ChevronDownIcon } from "lucide-react"
import { useNavigate, useSearchParams } from "react-router-dom"

import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ROLE_LABELS } from "@/lib/labels"
import { sessionPanelTo } from "@/routes/paths"

import {
  ALL,
  LIVE,
  ROLE_PARAM,
  ROLES,
  roleLabel,
  STATUS_PARAM,
  STATUSES,
  statusFilters,
  statusLabel,
} from "./filters"
import type { SessionListFilters } from "./queries"
import { SESSION_STATUS_META } from "./session-display"
import { SessionsList } from "./sessions-list"
import { useSessionFilters } from "./use-session-filters"

export function SessionsPage() {
  const [search] = useSearchParams()
  const navigate = useNavigate()
  const { status, role, filterBy } = useSessionFilters()

  const filters: SessionListFilters = { ...statusFilters(status), role: role ?? undefined }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Sessions"
        description="Every agent Ariadne has run, and the goal or task it was run for. Pick one to watch its terminal."
        actions={
          <>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by status"
                    className="w-40 justify-between font-normal"
                  />
                }
              >
                {statusLabel(status)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-40">
                <DropdownMenuRadioGroup
                  value={status ?? ALL}
                  onValueChange={(value) => filterBy(STATUS_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All statuses</DropdownMenuRadioItem>
                  {/* Above the individual statuses rather than among them: it
                      is three of them at once, and it is the one this screen
                      is usually opened for. */}
                  <DropdownMenuRadioItem value={LIVE}>Live</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {STATUSES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {SESSION_STATUS_META[known].label}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>

            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    aria-label="Filter by role"
                    className="w-36 justify-between font-normal"
                  />
                }
              >
                {roleLabel(role)}
                <ChevronDownIcon className="text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-36">
                <DropdownMenuRadioGroup
                  value={role ?? ALL}
                  onValueChange={(value) => filterBy(ROLE_PARAM, value)}
                >
                  <DropdownMenuRadioItem value={ALL}>All roles</DropdownMenuRadioItem>
                  <DropdownMenuSeparator />
                  {ROLES.map((known) => (
                    <DropdownMenuRadioItem key={known} value={known}>
                      {ROLE_LABELS[known]}
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />

      <SessionsList
        filters={filters}
        onSelect={(session) => void navigate(sessionPanelTo(search, session.id))}
      />
    </div>
  )
}
