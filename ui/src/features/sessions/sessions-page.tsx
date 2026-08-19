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
 * The two filters live in the URL, next to `?session=`: a hash-router desktop
 * app reloads often enough that component state would silently drop them, and
 * a narrowed screen is worth linking to. They replace rather than push, the way
 * the goals board's own filter does — a filter is not a place, and Back should
 * leave the screen rather than walk the selections that got here.
 */

import { ChevronDownIcon } from "lucide-react"
import { useNavigate, useSearchParams } from "react-router-dom"

import type { Role, SessionStatus } from "@/api"
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

import type { SessionListFilters } from "./queries"
import { SESSION_STATUS_META } from "./session-display"
import { SessionsList } from "./sessions-list"

/** The params the two filters travel in, alongside `?session=`. */
const STATUS_PARAM = "status"
const ROLE_PARAM = "role"

/** No filter, in both dropdowns: the value a param that is absent stands for. */
const ALL = "all"

/**
 * The one choice the daemon cannot answer on its own: the three statuses a
 * session with a live pane can be in. `GET /v1/sessions` takes a single status,
 * so this one is narrowed client-side — see `SessionListFilters.live`.
 */
const LIVE = "live"

/** Every status, in the order the badge ramp declares them (live ones first). */
const STATUSES = Object.keys(SESSION_STATUS_META) as SessionStatus[]

const ROLES = Object.keys(ROLE_LABELS) as Role[]

/** `?status=` as the list's filters: `live` here, anything else at the daemon. */
function statusFilters(value: string | null): Pick<SessionListFilters, "status" | "live"> {
  if (value === LIVE) return { live: true }
  const status = STATUSES.find((known) => known === value)
  return status ? { status } : {}
}

/** What the status trigger says. */
function statusLabel(value: string | null): string {
  if (value === LIVE) return "Live"
  const status = STATUSES.find((known) => known === value)
  return status ? SESSION_STATUS_META[status].label : "All statuses"
}

export function SessionsPage() {
  const [search, setSearch] = useSearchParams()
  const navigate = useNavigate()

  const status = search.get(STATUS_PARAM)
  const role = ROLES.find((known) => known === search.get(ROLE_PARAM))
  const filters: SessionListFilters = { ...statusFilters(status), role }

  /** Apply one filter, keeping every other param — an open panel survives it. */
  function filterBy(param: string, value: string) {
    const next = new URLSearchParams(search)
    if (value === ALL) next.delete(param)
    else next.set(param, value)
    setSearch(next, { replace: true })
  }

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
                {role ? ROLE_LABELS[role] : "All roles"}
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
