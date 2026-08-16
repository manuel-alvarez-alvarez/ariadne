/**
 * `ariadne session ls` as a screen: every agent of every goal, narrowed by
 * status and role.
 *
 * The filters live in the URL rather than in state, so a reload keeps them, and
 * so "the reviewers that failed" is a link somebody can be sent. They sit next
 * to the panel params on purpose — picking a session opens it in its task's
 * panel *over* this screen, which is why the row it came from is worth
 * highlighting.
 *
 * The table is {@link SessionsList}, the same one both panels' session tabs
 * are; this screen only decides what it is filtered to and what a pick means.
 */

import { useNavigate, useSearchParams } from "react-router-dom"

import type { Role, SessionDto, SessionStatus } from "@/api"
import { PageHeader } from "@/components/page-header"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { paths, taskSessionPanelTo } from "@/routes/paths"

import type { SessionListFilters } from "./queries"
import {
  ROLE_LABELS,
  ROLES,
  SESSION_STATUS_META,
  SESSION_STATUSES,
  sessionStatusLabel,
} from "./session-display"
import { SessionsList } from "./sessions-list"

/** Sentinel for "no filter" — `Select` needs a value for every item. */
const ALL = "all"

const STATUS_ITEMS = [
  { label: "All statuses", value: ALL },
  ...SESSION_STATUSES.map((status) => ({ label: sessionStatusLabel(status), value: status })),
]

const ROLE_ITEMS = [
  { label: "All roles", value: ALL },
  ...ROLES.map((role) => ({ label: ROLE_LABELS[role], value: role })),
]

export function SessionsPage() {
  const [search, setSearch] = useSearchParams()
  const navigate = useNavigate()

  // A hand-typed or stale `?status=`/`?role=` is ignored rather than sent to
  // the daemon, which would answer 400 for the whole list.
  const status = asStatus(search.get("status"))
  const role = asRole(search.get("role"))
  const filters: SessionListFilters = { status, role }

  /** Filters change; the panel params and everything else on the URL stay. */
  function setFilter(name: "status" | "role", value: string) {
    const next = new URLSearchParams(search)
    if (value === ALL) next.delete(name)
    else next.set(name, value)
    setSearch(next)
  }

  /**
   * A picked session opens where the rest of the app shows sessions: inside its
   * task's panel. A planner session has no task, and the goal panel only exists
   * on the board, so that one navigates there.
   */
  function open(session: SessionDto) {
    void navigate(
      session.task_id
        ? taskSessionPanelTo(search, session.task_id, session.id)
        : paths.goalSession(session.goal_id, session.id),
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Sessions"
        description="Every agent Ariadne has run, across all goals."
        actions={
          <>
            <FilterSelect
              label="Filter by status"
              value={status ?? ALL}
              items={STATUS_ITEMS}
              onChange={(value) => setFilter("status", value)}
            />
            <FilterSelect
              label="Filter by role"
              value={role ?? ALL}
              items={ROLE_ITEMS}
              onChange={(value) => setFilter("role", value)}
            />
          </>
        }
      />

      {/* The open panel's `?session=` is what marks the row it came from;
          `SessionsList` reads it itself. */}
      <SessionsList filters={filters} onSelect={open} />
    </div>
  )
}

function FilterSelect({
  label,
  value,
  items,
  onChange,
}: {
  label: string
  value: string
  items: { label: string; value: string }[]
  onChange: (value: string) => void
}) {
  return (
    <Select value={value} onValueChange={(next) => onChange((next as string) ?? ALL)} items={items}>
      <SelectTrigger aria-label={label} className="w-40">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

function asStatus(value: string | null): SessionStatus | undefined {
  return value !== null && value in SESSION_STATUS_META ? (value as SessionStatus) : undefined
}

function asRole(value: string | null): Role | undefined {
  return value !== null && value in ROLE_LABELS ? (value as Role) : undefined
}
