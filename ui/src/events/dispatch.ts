/**
 * The one place where daemon events meet the query cache.
 *
 * Two rules, applied to every event kind:
 *
 * 1. **Patch the detail.** Events are fat — they carry the whole updated DTO —
 *    so the entity's own `detail` key is written straight into the cache and an
 *    open detail screen updates without a round trip.
 * 2. **Invalidate the lists.** List responses are paginated and filtered, so
 *    they are refetched rather than patched. Only `<entity>.lists()` is
 *    invalidated, which by the key convention never disturbs detail entries.
 *    The sessions lists are the exception: they are patched from the event's
 *    row (see `patchSessionLists`).
 *
 * Screens therefore need no event handling of their own: read with the keys in
 * `src/api/query-keys.ts` and the cache stays live.
 *
 * `ai_permissions_updated` and `forge_settings_updated` have no list beside
 * their detail: each is one settings row, so the whole cache each moves is
 * `setQueryData`.
 *
 * Every host has its own stream, and an event patches the keys of the host it
 * came from (`[host, entity, …]`). `host_updated` is the settings daemon's
 * alone: it patches the row in `qk.hosts.list()`.
 *
 * Anything that arrives while a stream is down is lost — the daemon has no
 * replay. `invalidateEverything` is the recovery path and runs on every
 * reconnect and on every `resync`, for that stream's host.
 */

import type { QueryClient } from "@tanstack/react-query"

import {
  type DomainEvent,
  type Host,
  type HostDto,
  hostBaseUrl,
  LOCAL_HOST,
  qk,
  rememberHosts,
  type SessionDto,
} from "@/api"

/**
 * Apply one domain event to the query cache, under the keys of the host whose
 * stream delivered it: ids are a daemon's own, so an event only ever moves
 * its own host's entries.
 */
export function dispatchDomainEvent(
  queryClient: QueryClient,
  host: Host,
  event: DomainEvent,
): void {
  switch (event.event) {
    case "attention_changed": {
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "pull_requests_changed": {
      // The event carries no request, so every list and detail reads the
      // daemon's newest held fetch or row read again.
      void queryClient.invalidateQueries({ queryKey: qk.pullRequests.all(host) })
      // A reviewer session's own give-up, set or cleared, is a recovery
      // item's own evidence (`PullRequestRow::reviewer_given_up_at`).
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "goal_created": {
      queryClient.setQueryData(qk.goals.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists(host) })
      break
    }
    case "goal_updated": {
      queryClient.setQueryData(qk.goals.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists(host) })
      // The orchestrator's own confirmed-turn and give-up evidence rides on
      // this same event (`Goal::orchestrator_answered_failed_task_ids`,
      // `Goal::orchestrator_given_up_at`) — both a failed task's own item
      // and the taskless orchestrator's own item can turn on it.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "goal_deleted": {
      // The goal and everything under it are gone in one write, so its own
      // caches go with it; tasks and sessions are listed goal-first, so those
      // lists are refetched rather than pruned entry by entry.
      queryClient.removeQueries({ queryKey: qk.goals.detail(host, event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists(host) })
      void queryClient.invalidateQueries({ queryKey: qk.tasks.all(host) })
      void queryClient.invalidateQueries({ queryKey: qk.sessions.all(host) })
      // A deleted goal can take a recovery item's affected task with it.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "task_created": {
      queryClient.setQueryData(qk.tasks.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists(host) })
      break
    }
    case "task_updated": {
      const { task, transition } = event.data
      queryClient.setQueryData(qk.tasks.detail(host, task.id), task)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists(host) })
      if (transition) {
        void queryClient.invalidateQueries({ queryKey: qk.tasks.transitions(host, task.id) })
        // Landing the task is a transition too, and the diff endpoint answers
        // for the merge commit once there is one: what it returned before the
        // status moved is no longer what it would return now.
        void queryClient.invalidateQueries({ queryKey: qk.tasks.diff(host, task.id) })
      }
      // A failed task is a machine-resource recovery item's own evidence.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "task_branch_updated": {
      // A commit in the author's worktree. Nothing about the task row itself
      // changed — only the diff it would answer with.
      void queryClient.invalidateQueries({ queryKey: qk.tasks.diff(host, event.data.task_id) })
      break
    }
    case "message_sent": {
      // A message about the goal itself belongs to no task's channel.
      if (event.data.task_id) {
        void queryClient.invalidateQueries({
          queryKey: qk.tasks.messages(host, event.data.task_id),
        })
      }
      break
    }
    case "session_created": {
      queryClient.setQueryData(qk.sessions.detail(host, event.data.id), event.data)
      patchSessionLists(queryClient, host, event.data)
      invalidateWatchedPullRequest(queryClient, host, event.data.pull_request_id)
      break
    }
    case "session_updated": {
      queryClient.setQueryData(qk.sessions.detail(host, event.data.id), event.data)
      patchSessionLists(queryClient, host, event.data)
      // An `exhausted` flag's own recovery state is a quota item's evidence.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      invalidateWatchedPullRequest(queryClient, host, event.data.pull_request_id)
      break
    }
    case "agent_event": {
      void queryClient.invalidateQueries({ queryKey: qk.agentEvents.lists(host) })
      break
    }
    case "skill_created": {
      queryClient.setQueryData(qk.skills.detail(host, event.data.name), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.skills.lists(host) })
      break
    }
    case "skill_updated": {
      queryClient.setQueryData(qk.skills.detail(host, event.data.name), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.skills.lists(host) })
      break
    }
    case "skill_deleted": {
      queryClient.removeQueries({ queryKey: qk.skills.detail(host, event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.skills.lists(host) })
      break
    }
    case "workflow_created":
    case "workflow_updated": {
      queryClient.setQueryData(qk.workflows.detail(host, event.data.name), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.workflows.lists(host) })
      break
    }
    case "workflow_deleted": {
      queryClient.removeQueries({ queryKey: qk.workflows.detail(host, event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.workflows.lists(host) })
      break
    }
    case "repository_created": {
      queryClient.setQueryData(qk.repositories.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists(host) })
      break
    }
    case "repository_updated": {
      queryClient.setQueryData(qk.repositories.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists(host) })
      // Goals reference repositories live and carry them inline as
      // `GoalDto.repos`, so an edited path or base branch is stale in every
      // goal that works in it until the goals are read again. This is the one
      // case that reaches outside its own entity, and the reason it has to.
      void queryClient.invalidateQueries({ queryKey: qk.goals.all(host) })
      // A forge fetch error is a configuration recovery item's own evidence.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "repository_deleted": {
      queryClient.removeQueries({ queryKey: qk.repositories.detail(host, event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists(host) })
      // A deleted repository can take a configuration recovery item with it.
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
      break
    }
    case "ai_permissions_updated": {
      // One row, no list: the whole cache the Permissions screen reads.
      queryClient.setQueryData(qk.permissions.ai(host), event.data)
      break
    }
    case "learned_permission_created":
    case "learned_permission_updated": {
      queryClient.setQueryData(qk.learnedPermissions.detail(host, event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.learnedPermissions.lists(host) })
      break
    }
    case "learned_permission_deleted": {
      queryClient.removeQueries({ queryKey: qk.learnedPermissions.detail(host, event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.learnedPermissions.lists(host) })
      break
    }
    case "forge_settings_updated": {
      // One row, no list: the tunnel the settings dialog and the repositories screen read.
      queryClient.setQueryData(qk.forge.tunnel(host), event.data)
      break
    }
    case "issues_changed": {
      // Issues are read off the forge, not stored: the event carries no
      // issue, only the repository whose open issues moved, so its lists are
      // read again — both assignment filters.
      void queryClient.invalidateQueries({
        queryKey: qk.issues.ofRepository(host, event.data.repository_id),
      })
      break
    }
    case "dashboard_created":
    case "dashboard_updated": {
      queryClient.setQueryData(qk.dashboards.detail(host, event.data.name), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.dashboards.lists(host) })
      break
    }
    case "dashboard_deleted": {
      // Fat like the other two dashboard events — the daemon sends the
      // whole `DashboardDto` here too, not the generic `DeletedDto` every
      // other `*_deleted` event carries, since dashboards have no id of
      // their own for one to name; only `name` is read, to drop the row.
      queryClient.removeQueries({ queryKey: qk.dashboards.detail(host, event.data.name) })
      void queryClient.invalidateQueries({ queryKey: qk.dashboards.lists(host) })
      break
    }
    case "host_deleted": {
      if (host !== LOCAL_HOST) break
      const rows = queryClient.setQueryData<HostDto[]>(qk.hosts.list(), (rows) =>
        rows?.filter((row) => row.name !== event.data.name),
      )
      if (rows) rememberHosts(rows)
      queryClient.removeQueries({ queryKey: qk.host(event.data.name) })
      break
    }
    case "host_updated": {
      // The hosts are the settings daemon's configuration; another host's
      // own hosts are none of this app's business.
      if (host !== LOCAL_HOST) break
      const before = hostBaseUrl(event.data.name)
      const rows = queryClient.setQueryData<HostDto[]>(qk.hosts.list(), (rows) =>
        rows?.some((row) => row.name === event.data.name)
          ? rows.map((row) => (row.name === event.data.name ? event.data : row))
          : [...(rows ?? []), event.data],
      )
      if (rows) rememberHosts(rows)
      // A host that came up, or moved, answers every read it failed before.
      const after = hostBaseUrl(event.data.name)
      if (after !== null && after !== before) {
        void queryClient.invalidateQueries({ queryKey: qk.host(event.data.name) })
      }
      break
    }
    default: {
      // A kind the generated types do not know about: the daemon is newer than
      // these types. Regenerate with `npm run gen:api`.
      const unknown: never = event
      console.warn("[events] unhandled domain event", unknown)
    }
  }
}

/**
 * A pull request session that starts or moves: the request names its newest
 * session (`PullRequestDto.session_id`), so its detail and every list holding
 * it are read again. Nothing for any other session.
 */
function invalidateWatchedPullRequest(
  queryClient: QueryClient,
  host: Host,
  pullRequestId: string | null | undefined,
): void {
  if (!pullRequestId) return
  void queryClient.invalidateQueries({ queryKey: qk.pullRequests.detail(host, pullRequestId) })
  void queryClient.invalidateQueries({ queryKey: qk.pullRequests.lists(host) })
}

/** The daemon-side filters a sessions list key carries (`SessionListFilters`). */
interface SessionListKeyFilters {
  goal?: string
  task?: string
  pull_request?: string
  status?: string
}

/**
 * Write a started or moved session into every cached sessions list, in place.
 *
 * This is the one list the dispatcher patches rather than refetches. A list
 * holds every session the host ever ran (`all: true`), so a refetch reads the
 * whole history page by page, and sessions move several times a second while
 * agents work. Refetching on each move kept the daemon answering the same
 * listing without end. The event carries the whole row, and the list is
 * narrowed only by the fields on that row, so the patch is exact. The row
 * goes where the daemon would put it: newest activity first.
 *
 * A list that is still being read is refetched instead, because the read that
 * is under way would land after the patch and put the older row back.
 */
function patchSessionLists(queryClient: QueryClient, host: Host, session: SessionDto): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: qk.sessions.lists(host) })) {
    if (query.state.fetchStatus === "fetching") {
      void queryClient.invalidateQueries({ queryKey: query.queryKey, exact: true })
      continue
    }
    const rows = query.state.data
    if (!Array.isArray(rows)) continue
    const filters = (query.queryKey[3] ?? {}) as SessionListKeyFilters
    const others = (rows as SessionDto[]).filter((row) => row.id !== session.id)
    const next = belongsIn(session, filters) ? [...others, session].sort(newestFirst) : others
    if (next.length === others.length && others.length === rows.length) continue
    queryClient.setQueryData(query.queryKey, next)
  }
}

function belongsIn(session: SessionDto, filters: SessionListKeyFilters): boolean {
  return (
    (!filters.goal || session.goal_id === filters.goal) &&
    (!filters.task || session.task_id === filters.task) &&
    (!filters.pull_request || session.pull_request_id === filters.pull_request) &&
    (!filters.status || session.status === filters.status)
  )
}

/** The daemon's listing order: last activity, newest first, then the id. */
function newestFirst(left: SessionDto, right: SessionDto): number {
  const activity = (session: SessionDto) =>
    Date.parse(session.last_activity_at ?? session.created_at) || 0
  return activity(right) - activity(left) || left.id.localeCompare(right.id)
}

/**
 * Drop every assumption about one host's state: used after a gap in that
 * host's stream (reconnect, `resync`) where arbitrary events may have been
 * missed. Every other host's stream is still live, so its entries stay.
 */
export function invalidateEverything(queryClient: QueryClient, host: Host): void {
  void queryClient.invalidateQueries({ queryKey: qk.host(host) })
}
