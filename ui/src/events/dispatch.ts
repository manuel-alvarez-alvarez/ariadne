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
 *
 * Screens therefore need no event handling of their own: read with the keys in
 * `src/api/query-keys.ts` and the cache stays live.
 *
 * Anything that arrives while the stream is down is lost — the daemon has no
 * replay. `invalidateEverything` is the recovery path and runs on every
 * reconnect and on every `resync`.
 */

import type { QueryClient } from "@tanstack/react-query"

import { type DomainEvent, qk } from "@/api"

/** Apply one domain event to the query cache. */
export function dispatchDomainEvent(queryClient: QueryClient, event: DomainEvent): void {
  switch (event.event) {
    case "goal_created": {
      queryClient.setQueryData(qk.goals.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      break
    }
    case "goal_updated": {
      queryClient.setQueryData(qk.goals.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      break
    }
    case "goal_deleted": {
      // The goal and everything under it are gone in one write, so its own
      // caches go with it; tasks and sessions are listed goal-first, so those
      // lists are refetched rather than pruned entry by entry.
      queryClient.removeQueries({ queryKey: qk.goals.detail(event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.goals.lists() })
      void queryClient.invalidateQueries({ queryKey: qk.tasks.all() })
      void queryClient.invalidateQueries({ queryKey: qk.sessions.all() })
      break
    }
    case "task_created": {
      queryClient.setQueryData(qk.tasks.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
      break
    }
    case "task_updated": {
      const { task, transition } = event.data
      queryClient.setQueryData(qk.tasks.detail(task.id), task)
      void queryClient.invalidateQueries({ queryKey: qk.tasks.lists() })
      if (transition) {
        void queryClient.invalidateQueries({ queryKey: qk.tasks.transitions(task.id) })
        // Landing the task is a transition too, and the diff endpoint answers
        // for the merge commit once there is one: what it returned before the
        // status moved is no longer what it would return now.
        void queryClient.invalidateQueries({ queryKey: qk.tasks.diff(task.id) })
      }
      break
    }
    case "task_branch_updated": {
      // A commit in the engineer's worktree. Nothing about the task row itself
      // changed — only the diff it would answer with.
      void queryClient.invalidateQueries({ queryKey: qk.tasks.diff(event.data.task_id) })
      break
    }
    case "message_created": {
      // A message belongs either to a task thread or to the goal's plan thread.
      const key = event.data.task_id
        ? qk.tasks.messages(event.data.task_id)
        : qk.goals.messages(event.data.goal_id)
      void queryClient.invalidateQueries({ queryKey: key })
      break
    }
    case "review_created": {
      void queryClient.invalidateQueries({ queryKey: qk.tasks.reviews(event.data.task_id) })
      break
    }
    case "session_created": {
      queryClient.setQueryData(qk.sessions.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.sessions.lists() })
      break
    }
    case "session_updated": {
      queryClient.setQueryData(qk.sessions.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.sessions.lists() })
      break
    }
    case "agent_event": {
      void queryClient.invalidateQueries({ queryKey: qk.agentEvents.lists() })
      break
    }
    case "profile_created": {
      queryClient.setQueryData(qk.profiles.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.profiles.lists() })
      break
    }
    case "profile_updated": {
      queryClient.setQueryData(qk.profiles.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.profiles.lists() })
      break
    }
    case "profile_deleted": {
      queryClient.removeQueries({ queryKey: qk.profiles.detail(event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.profiles.lists() })
      break
    }
    case "repository_created": {
      queryClient.setQueryData(qk.repositories.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists() })
      break
    }
    case "repository_updated": {
      queryClient.setQueryData(qk.repositories.detail(event.data.id), event.data)
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists() })
      // Goals reference repositories live and carry them inline as
      // `GoalDto.repos`, so an edited path or base branch is stale in every
      // goal that works in it until the goals are read again. This is the one
      // case that reaches outside its own entity, and the reason it has to.
      void queryClient.invalidateQueries({ queryKey: qk.goals.all() })
      break
    }
    case "repository_deleted": {
      queryClient.removeQueries({ queryKey: qk.repositories.detail(event.data.id) })
      void queryClient.invalidateQueries({ queryKey: qk.repositories.lists() })
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
 * Drop every assumption about daemon state: used after a gap in the stream
 * (reconnect, `resync`) where arbitrary events may have been missed.
 */
export function invalidateEverything(queryClient: QueryClient): void {
  void queryClient.invalidateQueries()
}
