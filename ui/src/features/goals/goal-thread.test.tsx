// @vitest-environment jsdom

/**
 * The planner thread: who its compose box lets the user address, and where in
 * the conversation it puts them.
 *
 * Only the goal's planner works in this thread, and that is the whole rule the
 * daemon applies to it (`http/recipients.rs`): engineers and reviewers are
 * addressed in the task threads they work in, where which task is meant is not
 * in question. So the picker offers one name, however many profiles exist.
 *
 * The rest is what a long thread does. A thread is read from its newest
 * message, so opening it puts the panel on the end; a message that arrives
 * while the reader is further up is counted rather than jumped to, and the
 * pill is the way back down. None of that is layout jsdom has — the thread has
 * no scroll of its own, the panel around it is the scroll container — so the
 * panel is a `overflow-y: auto` box whose measurements this file writes, and
 * what is pinned is what the thread asks of it.
 *
 * Everything else is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, not this one's. A message arriving is that cache being
 * written, which is exactly what the stream's `message_created` does to it.
 */

import { act, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import { type GoalDto, type MessageDto, type ProfileDto, qk } from "@/api"
import { aGoal, aMessage } from "@/test/fixtures"
import { renderScreen } from "@/test/harness"
import { GoalThread } from "./goal-thread"

function profile(id: string, name: string, role: ProfileDto["role"]): ProfileDto {
  return {
    id,
    name,
    role,
    system_prompt: "",
    system_prompt_is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  }
}

const PROFILES: ProfileDto[] = [
  profile("01PLANNER", "Planner", "planner"),
  profile("01ENGINEER", "Builder", "engineer"),
  profile("01REVIEWER", "Strict", "reviewer"),
]

const GOAL: GoalDto = aGoal({
  id: "01GOAL",
  title: "Ship it",
  description: "Ship it, all of it",
  status: "planning",
  planner_profile_id: "01PLANNER",
})

/** A thread long enough that where it opens is the difference between screens. */
const THREAD: MessageDto[] = Array.from({ length: 60 }, (_, index) =>
  aMessage({
    id: `01JMESG${String(index).padStart(19, "0")}`,
    goal_id: GOAL.id,
    author_role: "planner",
    body: `message ${index}`,
  }),
)

const OTHER_GOAL: GoalDto = aGoal({
  id: "01OTHERGOAL",
  title: "Something else",
  status: "active",
  planner_profile_id: "01PLANNER",
})

/**
 * The other goal's thread. Its ids sort *above* the first goal's, which is what
 * makes the switch a real question: a view that kept the last thread's place
 * would read every one of these as newer than what the reader had seen.
 */
const OTHER_THREAD: MessageDto[] = [1, 2, 3].map((n) =>
  aMessage({
    id: `01ZMESG000000000000000000${n}`,
    goal_id: OTHER_GOAL.id,
    author_role: "planner",
    body: `the other thread ${n}`,
  }),
)

/** How far the panel scrolls, and how much of it is on screen. */
const PANEL_HEIGHT = 2_000
const VIEWPORT = 500

/** Every `scrollTo` the thread asked its panel for, in order. */
let scrolled: number[] = []

beforeEach(() => {
  scrolled = []
  localStorage.clear()
  // jsdom lays nothing out and scrolls nothing; the thread is what asks, and
  // what it asks for is the assertion. Written on the prototype because the
  // thread measures its panel while it mounts, which is before a test could
  // reach the node.
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
    value: PANEL_HEIGHT,
    configurable: true,
  })
  Object.defineProperty(HTMLElement.prototype, "clientHeight", {
    value: VIEWPORT,
    configurable: true,
  })
  Element.prototype.scrollTo = vi.fn((options?: ScrollToOptions | number) => {
    if (typeof options === "object" && options?.top !== undefined) scrolled.push(options.top)
  }) as typeof Element.prototype.scrollTo
})

/**
 * The thread inside the one thing it needs from a browser: a box that scrolls,
 * as tall as a panel and showing a screenful of it.
 */
function mount(messages: MessageDto[] = THREAD) {
  const inPanel = (goalId: string) => (
    <div data-testid="panel" style={{ overflowY: "auto" }}>
      <GoalThread goalId={goalId} />
    </div>
  )
  const { queryClient, rerender } = renderScreen(inPanel(GOAL.id), {
    seed: (client) => {
      client.setQueryData(qk.goals.detail(GOAL.id), GOAL)
      client.setQueryData(qk.goals.messages(GOAL.id), messages)
      client.setQueryData(qk.profiles.list({}), PROFILES)
    },
  })
  return {
    queryClient,
    panel: screen.getByTestId("panel"),
    /**
     * Point the same panel at another goal's thread. Its rows go into the cache
     * here rather than in the seed above: nothing observes them until the
     * switch, and the harness collects an unobserved query on the spot.
     */
    showGoal: (goal: GoalDto, thread: MessageDto[]) => {
      queryClient.setQueryData(qk.goals.detail(goal.id), goal)
      queryClient.setQueryData(qk.goals.messages(goal.id), thread)
      rerender(inPanel(goal.id))
    },
  }
}

/** The panel, scrolled to wherever the reader left it. */
function scrollTo(panel: HTMLElement, top: number) {
  panel.scrollTop = top
  act(() => {
    panel.dispatchEvent(new Event("scroll"))
  })
}

/**
 * The daemon says something, the way `message_created` reaches the cache — and
 * the wait for the thread to have drawn it, which react-query schedules rather
 * than doing on the write.
 */
async function arrives(
  queryClient: ReturnType<typeof mount>["queryClient"],
  messages: MessageDto[],
  body: string,
): Promise<MessageDto[]> {
  const next = [
    ...messages,
    aMessage({ id: `01JMESGZ${messages.length}`, goal_id: GOAL.id, author_role: "planner", body }),
  ]
  act(() => {
    queryClient.setQueryData(qk.goals.messages(GOAL.id), next)
  })
  await screen.findByText(body)
  return next
}

it("offers the goal's planner, and no one who works in a task thread", async () => {
  mount([])
  const user = userEvent.setup()

  await user.click(screen.getByRole("combobox", { name: "Addressee" }))
  const options = (await screen.findAllByRole("option")).map((option) => option.textContent)

  expect(options).toEqual(["the thread", "Planner"])
})

it("opens on the newest message of a long thread", () => {
  mount()

  // The whole thread is drawn — it is the panel that scrolls, not the list —
  // and the panel is put on its end rather than its beginning.
  expect(screen.getByText("message 0")).toBeTruthy()
  expect(screen.getByText("message 59")).toBeTruthy()
  expect(scrolled).toEqual([PANEL_HEIGHT])
})

it("follows the thread while the reader is on the newest message", async () => {
  const { queryClient, panel } = mount()
  scrollTo(panel, PANEL_HEIGHT - VIEWPORT)

  await arrives(queryClient, THREAD, "and one more thing")

  // Scrolled again, to the message that just landed: no pill, nothing to catch
  // up on.
  expect(scrolled).toEqual([PANEL_HEIGHT, PANEL_HEIGHT])
  expect(screen.queryByRole("button", { name: /new message/i })).toBeNull()
})

it("counts what arrives while the reader is further up, and takes them to it", async () => {
  const { queryClient, panel } = mount()
  scrollTo(panel, 0)

  await arrives(queryClient, THREAD, "while you were reading")

  // Not dragged down: the message is offered instead.
  expect(scrolled).toEqual([PANEL_HEIGHT])
  const pill = screen.getByRole("button", { name: "1 new message" })

  await userEvent.setup().click(pill)

  expect(scrolled).toEqual([PANEL_HEIGHT, PANEL_HEIGHT])
  expect(screen.queryByRole("button", { name: /new message/i })).toBeNull()
})

it("counts every message that landed up there, not just the last one", async () => {
  const { queryClient, panel } = mount()
  scrollTo(panel, 0)

  const one = await arrives(queryClient, THREAD, "one")
  await arrives(queryClient, one, "two")

  expect(screen.getByRole("button", { name: "2 new messages" })).toBeTruthy()
})

it("closes the box on a goal that is over", () => {
  renderScreen(<GoalThread goalId={GOAL.id} />, {
    seed: (client) => {
      client.setQueryData(qk.goals.detail(GOAL.id), { ...GOAL, status: "cancelled" })
      client.setQueryData(qk.goals.messages(GOAL.id), [])
      client.setQueryData(qk.profiles.list({}), PROFILES)
    },
  })

  const box = screen.getByRole("textbox", { name: "Message the planner thread" })
  expect((box as HTMLTextAreaElement).disabled).toBe(true)
  expect(screen.getByText("Cancelled: no planner is left to read this.")).toBeTruthy()
})

/**
 * A panel can be pointed at another thread without ever closing — the stacked
 * task panel does exactly that. What it lands on is a thread being opened, and
 * the place the reader had in the last one is not theirs to keep.
 */
it("opens the next thread it is pointed at, rather than carrying the last one's place", async () => {
  const { queryClient, panel, showGoal } = mount()
  scrollTo(panel, 0)
  await arrives(queryClient, THREAD, "while you were reading")
  expect(screen.getByRole("button", { name: "1 new message" })).toBeTruthy()

  const before = scrolled.length
  showGoal(OTHER_GOAL, OTHER_THREAD)

  expect(screen.getByText("the other thread 3")).toBeTruthy()
  // Not "everything here newer than a message from another conversation".
  expect(screen.queryByRole("button", { name: /new message/i })).toBeNull()
  // And it opens on its end, like any thread being opened.
  expect(scrolled.length).toBe(before + 1)
  expect(scrolled.at(-1)).toBe(PANEL_HEIGHT)
})
