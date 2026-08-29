// @vitest-environment jsdom

/**
 * The diff tab keeping up with the branch on its own.
 *
 * It carried a Refresh button until the daemon learned to say when a task's
 * branch head moved; now the only thing between a commit in the engineer's
 * worktree and this view is the stream. `dispatch.test.ts` pins what the event
 * does to the cache — this pins the whole path: a daemon pushing the event down
 * the app's one `EventSource`, and the diff on screen refetching with nothing
 * clicked. It is driven from the socket rather than from `dispatchDomainEvent`
 * for exactly that reason: the parts between them (the kind being one the
 * stream forwards at all, the provider handing it to the cache) are where this
 * would silently stop working.
 */

import { act, screen, waitFor } from "@testing-library/react"
import { beforeEach, expect, it } from "vitest"

import type { DomainEvent } from "@/api"
import { EventStreamProvider } from "@/events/provider"
import { type FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"
import { aTask } from "@/test/fixtures"
import { daemonFetch, renderScreen } from "@/test/harness"
import { TaskDiff } from "./task-diff"

const TASK = aTask()

/** The commit the branch moved to; the sha itself is not read by anything. */
const HEAD = "1ea7fca11ab1e0000000000000000000000000de"

const FIRST = `diff --git a/one.txt b/one.txt
--- a/one.txt
+++ b/one.txt
@@ -1 +1 @@
-old
+new
`

const SECOND = `${FIRST}diff --git a/two.txt b/two.txt
new file mode 100644
--- /dev/null
+++ b/two.txt
@@ -0,0 +1 @@
+just committed
`

/** What `GET /v1/tasks/{id}/diff` answers next; the test moves it on. */
let diff = FIRST

/** The daemon, answering the one request the tab makes. */
function stubDaemon() {
  daemonFetch.mockImplementation(() =>
    Promise.resolve(new Response(diff, { headers: { "content-type": "text/plain" } })),
  )
}

beforeEach(() => {
  diff = FIRST
  stubDaemon()
  stubEventSource()
})

/** How many times the diff has been asked for. */
function diffRequests(): number {
  return daemonFetch.mock.calls.filter(([input]) =>
    new URL(typeof input === "string" ? input : (input as Request).url).pathname.endsWith("/diff"),
  ).length
}

/**
 * The tab under the app's own stream, with the daemon's end of it in hand:
 * what is returned pushes an event the way the daemon would.
 */
async function renderDiff(): Promise<(event: DomainEvent) => void> {
  renderScreen(
    <EventStreamProvider>
      <TaskDiff taskId={TASK.id} />
    </EventStreamProvider>,
    { route: "/goals" },
  )
  expect(await screen.findByText("one.txt")).toBeTruthy()

  const stream: FakeEventSource = latestSource()
  act(() => {
    stream.succeed()
    stream.beat()
  })
  return (event) =>
    act(() => {
      stream.emit(event.event, event.data)
    })
}

it("refetches when the daemon says the branch head moved", async () => {
  const dispatch = await renderDiff()
  expect(diffRequests()).toBe(1)
  diff = SECOND

  dispatch({
    event: "task_branch_updated",
    data: { task_id: TASK.id, goal_id: TASK.goal_id, branch: TASK.branch, head: HEAD },
  })

  // The file that the commit added is on screen, and nothing was clicked.
  expect(await screen.findByText("two.txt")).toBeTruthy()
})

it("leaves the diff of another task alone", async () => {
  const dispatch = await renderDiff()
  diff = SECOND

  dispatch({
    event: "task_branch_updated",
    data: {
      task_id: "01JTASK0000000000000OTHER1",
      goal_id: TASK.goal_id,
      branch: "some-other-task-000002",
      head: HEAD,
    },
  })

  await waitFor(() => expect(diffRequests()).toBe(1))
  expect(screen.queryByText("two.txt")).toBeNull()
})

it("refetches when the task itself transitions, since landing moves the diff", async () => {
  const dispatch = await renderDiff()
  diff = SECOND

  dispatch({
    event: "task_updated",
    data: {
      task: { ...TASK, status: "merged", merge_commit: HEAD },
      transition: {
        id: "01JTRAN0000000000000000001",
        actor: "daemon",
        from_status: "approved",
        to_status: "merged",
        created_at: TASK.updated_at,
      },
    },
  })

  expect(await screen.findByText("two.txt")).toBeTruthy()
})
