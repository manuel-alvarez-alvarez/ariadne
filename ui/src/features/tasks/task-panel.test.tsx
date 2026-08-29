// @vitest-environment jsdom

/**
 * What the panel says a task's agents run on.
 *
 * A profile is editable and a task is not: what each slot runs on lands on the
 * task and on each reviewer slot when the task is created, and that is what the
 * daemon launches from. The panel used to render the profile's live value
 * instead, so the one moment the question matters — somebody moved the profile
 * onto another model — was the moment the panel lied about work already under
 * way.
 *
 * So both rows are read here against profiles that have since moved, including
 * the two reviewer slots that share nothing but their order, and the slot with
 * no pin, which says `auto` rather than borrowing the profile's answer.
 *
 * The pull request row is here because it exists only sometimes.
 *
 * The tokens are here for a different reason: the panel shows the daemon's own
 * aggregate twice over — the task's total in the facts, and the split by who
 * spent it in the hint behind that total — and both have to be the figures the
 * daemon sent rather than anything added up here.
 *
 * The last two are about the panel fitting in 48rem: its header keeps the
 * actions on the title row whatever the status offers, and its sessions tab is
 * the folded four-column table rather than the screen's seven — the id on the
 * role's own line, and what the session spent behind its last activity.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, and the tabs are `task-panel.tsx`'s own.
 */

import { act, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import {
  type MessageDto,
  type ProfileDto,
  qk,
  type ReviewDto,
  type SessionDto,
  type TaskDto,
} from "@/api"
import { markThreadSeen } from "@/components/thread-unread"
import { shortId } from "@/lib/format"
import { aMessage, aSession } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

const ENGINEER = "01JPROF0000000000000000ENG"
const STRICT = "01JPROF0000000000000STRICT"
const AUTO = "01JPROF00000000000000AUTO"

/** Every profile as it stands *today* — all three edited since the task. */
const PROFILES: ProfileDto[] = [
  {
    id: ENGINEER,
    name: "Builder",
    role: "engineer",
    model: "opencode:grok-4",
    system_prompt: "",
    system_prompt_is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: STRICT,
    name: "Strict",
    role: "reviewer",
    model: "opencode:grok-4",
    system_prompt: "",
    system_prompt_is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
  {
    id: AUTO,
    name: "Second",
    role: "reviewer",
    model: "opencode:grok-4",
    system_prompt: "",
    system_prompt_is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  },
]

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: "01JGOAL0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  title: "Surface the pins",
  description: "",
  status: "in_progress",
  branch: "surface-the-pins-000001",
  depends_on: [],
  engineer_profile_id: ENGINEER,
  model: "codex:gpt-5",
  reviewers: [
    { profile_id: STRICT, model: "claude_code:claude-sonnet-5" },
    // Assigned with no model at all: the agent CLI is resolved at spawn time
    // and it takes that CLI's default. A pin like any other.
    { profile_id: AUTO, model: null },
  ],
  review_round: 0,
  stalled: false,
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    engineer: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    // Only the reviewer that has actually been spawned: the second slot has
    // never run, and the daemon lists no row for it.
    reviewers: [
      {
        profile_id: STRICT,
        profile_name: "Strict",
        usage: { input_tokens: 234_567, cached_input_tokens: 200_000, output_tokens: 5_300 },
      },
    ],
  },
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
}

/** The one session of the task, for the panel's sessions tab. */
const SESSION: SessionDto = aSession({
  id: "01JSESS0000000000000000ENG",
  task_id: TASK.id,
  goal_id: TASK.goal_id,
  profile_id: ENGINEER,
  usage: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
})

// How far into a thread the reader has got outlives a test, as it outlives a
// panel; each test below says where this one is starting from.
beforeEach(() => {
  localStorage.clear()
  // The conversation tab puts a thread on screen, and a thread scrolls the
  // panel it is in; jsdom has no scrolling of its own.
  Element.prototype.scrollTo = vi.fn()
})

function mount(task: TaskDto = TASK, messages?: MessageDto[]) {
  return renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
      client.setQueryData(qk.profiles.list({}), PROFILES)
      if (messages) client.setQueryData(qk.tasks.messages(task.id), messages)
    },
  })
}

/** One of the panel's tabs, by name. */
function tab(name: RegExp | string): HTMLElement {
  return screen.getByRole("tab", { name })
}

/**
 * An agent says something on the task, the way `message_created` reaches the
 * cache — with the turn react-query takes to tell the screen about it.
 */
async function arrives(
  queryClient: ReturnType<typeof mount>["queryClient"],
  messages: MessageDto[],
  id: string,
): Promise<MessageDto[]> {
  const next = [
    ...messages,
    aMessage({ id, task_id: TASK.id, goal_id: TASK.goal_id, author_role: "engineer", body: id }),
  ]
  await act(async () => {
    queryClient.setQueryData(qk.tasks.messages(TASK.id), next)
    // react-query schedules the notification rather than making it on the
    // write, so the turn it takes has to happen inside this act.
    await new Promise((settled) => setTimeout(settled, 0))
  })
  return next
}

/** A thread of three, the last two of them said since the reader last looked. */
const THREAD: MessageDto[] = ["01JMESG1", "01JMESG2", "01JMESG3"].map((id) =>
  aMessage({ id, task_id: TASK.id, goal_id: TASK.goal_id, author_role: "engineer", body: id }),
)

/** Two verdicts in one round: what the Reviews tab lists, and so what it counts. */
const REVIEWS: ReviewDto[] = [STRICT, AUTO].map((reviewer, index) => ({
  id: `01JREVW00000000000000000${index}`,
  task_id: TASK.id,
  reviewer_profile_id: reviewer,
  round: 0,
  verdict: "approve",
  created_at: "2026-01-01T00:00:00Z",
}))

/**
 * The task's sessions, answered by the daemon rather than seeded: the tab is
 * only mounted once it is clicked, and a seeded entry nothing observes is
 * collected before then. Everything else — the session behind a picked row
 * included — keeps the never-settling default, which is what leaves the panel
 * on its skeleton instead of mounting a terminal in a DOM that has no canvas.
 */
function stubSessions() {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    if (url.pathname === "/v1/sessions") return Promise.resolve(jsonResponse([SESSION]))
    return new Promise(() => {})
  })
}

/**
 * Both lists the tab strip counts, on the keys the tabs' own views read: the
 * counts are the daemon's answers to the requests those tabs already make, so
 * they arrive the same way — after a turn, rather than seeded.
 */
function stubTabLists(sessions: SessionDto[] = [SESSION], reviews: ReviewDto[] = REVIEWS) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    if (url.pathname === "/v1/sessions") return Promise.resolve(jsonResponse(sessions))
    if (url.pathname === `/v1/tasks/${TASK.id}/reviews`) {
      return Promise.resolve(jsonResponse(reviews))
    }
    return new Promise(() => {})
  })
}

/**
 * The value of a fact, by the label above it. The gap between a profile's name
 * and the two facts after it is a CSS one, so the text comes out run together;
 * one space is put back so the assertions read as the line does.
 */
function fact(label: string): string {
  const term = screen.getByText(label)
  const value = term.nextElementSibling
  if (!value) throw new Error(`no value under "${label}"`)
  return (value.textContent ?? "").replaceAll("·", " ·").replaceAll("  ·", " ·")
}

/** The hint behind a fact's figure, opened the way a keyboard opens it. */
async function hint(label: string): Promise<HTMLElement> {
  const figure = screen
    .getByText(label)
    .nextElementSibling?.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")
  if (!figure) throw new Error(`no figure under "${label}"`)
  figure.focus()
  const exact = await screen.findByText("Input")
  const popup = exact.closest<HTMLElement>("[data-slot='tooltip-content']")
  if (!popup) throw new Error("no hint around the exact counts")
  return popup
}

it("shows the engineer's pin, not what its profile says today", () => {
  mount()

  expect(fact("Engineer")).toContain("Builder")
  expect(fact("Engineer")).toContain("codex:gpt-5")
  expect(fact("Engineer")).not.toContain("grok-4")
})

it("shows each reviewer slot's own pin, in review order", () => {
  mount()

  const reviewers = fact("Reviewers")
  expect(reviewers).toContain("Strict · claude_code:claude-sonnet-5")
  // Both reviewer profiles now say `opencode:grok-4`; the slots do not.
  expect(reviewers).toContain("Second · auto")
  expect(reviewers).not.toContain("grok-4")
})

it("says a task has no reviewers rather than showing an empty list", () => {
  mount({ ...TASK, reviewers: [] })

  expect(fact("Reviewers")).toBe("none assigned")
})

it("links the pull request its engineer published", () => {
  mount({
    ...TASK,
    status: "approved",
    pr_url: "https://github.com/owner/repo/pull/12",
  })

  const link = screen.getByRole("link", { name: "https://github.com/owner/repo/pull/12" })
  expect(link.getAttribute("href")).toBe("https://github.com/owner/repo/pull/12")
})

it("leaves the pull request row out of a task landed locally", () => {
  mount()

  expect(screen.queryByText("Pull request")).toBeNull()
})

it("shows the task's total, as the daemon aggregated it", () => {
  mount()

  // The share is read off the exact counts, not the rounded halves beside
  // it: 1,100,000 of 1,234,567 is 89%, where 1.1M of 1.2M would say 92%.
  expect(fact("Tokens")).toBe("1.2M in, 89% cached, 45k out")
})

it("says zero for a task whose agents have reported nothing", () => {
  mount({
    ...TASK,
    usage: {
      total: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
      engineer: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
      reviewers: [],
    },
  })

  // A share of zero rather than a gap: nothing was sent, so nothing was
  // cached, and that is an answer.
  expect(fact("Tokens")).toBe("0 in, 0% cached, 0 out")
})

it("breaks the total down by the agent that spent it, reviewers named", async () => {
  mount()
  const popup = await hint("Tokens")

  // The engineer first, then the one reviewer that has actually run — named
  // by the daemon, since the figures are its own. The second slot has never
  // been spawned and is not a line at all.
  const who = [...popup.querySelectorAll("dt")].map((agent) => agent.textContent)
  expect(who).toEqual(["Engineer", "Strict"])
  expect(popup.textContent).not.toContain("Second")

  const figures = [...popup.querySelectorAll("dd")].map((figure) => figure.textContent)
  expect(figures).toEqual(["1M in, 90% cached, 40k out", "235k in, 85% cached, 5.3k out"])

  // The two halves lead the hint, named and each on its own line, in the same
  // rounded form the figure shows and carrying the task's own total rather
  // than the lines under it added up.
  const total = within(popup)
  const input = total.getByText("Input")
  expect(input.nextElementSibling?.textContent).toBe("1.2M")
  // The share rides beside the input count, part of it rather than a count of
  // its own — the same share the figure itself shows.
  expect(input.nextElementSibling?.nextElementSibling?.textContent).toBe("89%")
  expect(total.getByText("Output").nextElementSibling?.textContent).toBe("45k")
  // Nothing in the hint is spelled to the digit any more: not the halves, not
  // the rows under them.
  expect(popup.textContent).not.toMatch(/\d,\d/)
})

it("keeps the sessions tab to the sessions, with no breakdown above them", async () => {
  mount()
  await userEvent.setup().click(screen.getByRole("tab", { name: "Sessions" }))

  // The figure in the facts carries the total and its split; a card repeating
  // both above a table whose rows carry their own figures said it all twice.
  expect(screen.queryByRole("heading", { name: "Tokens" })).toBeNull()
})

/**
 * The panel opens on the description, so a thread that has moved on since the
 * reader last had it in front of them has to say so from the tab that leads to
 * it — otherwise the strip looks exactly as it did.
 */
it("counts what the conversation has gained since it was last read", () => {
  markThreadSeen(`task:${TASK.id}`, "01JMESG1")
  mount(TASK, THREAD)

  const trigger = screen.getByRole("tab", { name: /Conversation/ })
  expect(within(trigger).getByLabelText("2 unread messages").textContent).toBe("2")
})

it("says nothing about a thread the reader is up to date with", () => {
  markThreadSeen(`task:${TASK.id}`, "01JMESG3")
  mount(TASK, THREAD)

  const trigger = screen.getByRole("tab", { name: /Conversation/ })
  expect(within(trigger).queryByLabelText(/unread/)).toBeNull()
})

it("holds a thread nobody has opened yet to be read", () => {
  mount(TASK, THREAD)

  // A task picked off the board would otherwise announce its whole history as
  // new, which says nothing about what changed.
  expect(within(tab(/Conversation/)).queryByLabelText(/unread/)).toBeNull()
})

/**
 * The panel opens on its description, and can be closed again without the
 * thread ever having been drawn. Nothing here may record that the reader has
 * seen it — a mark written because a *panel* rendered would say they read
 * messages that were never on screen, and the count that follows would be off
 * by everything said before they first looked.
 */
it("records nothing about a thread the panel never showed", async () => {
  const { queryClient } = mount(TASK, THREAD)

  await arrives(queryClient, THREAD, "01JMESG4")

  expect(within(tab(/Conversation/)).queryByLabelText(/unread/)).toBeNull()
  // Not merely uncounted: nothing was written on the reader's behalf at all.
  expect(localStorage.length).toBe(0)
})

it("counts from where the reader left the thread, once they have been in it", async () => {
  const user = userEvent.setup()
  const { queryClient } = mount(TASK, THREAD)

  // Opening the thread is what says the reader has seen it.
  await user.click(tab(/Conversation/))
  await screen.findByText("01JMESG3")
  await user.click(tab("Description"))

  const said = await arrives(queryClient, THREAD, "01JMESG4")
  expect(within(tab(/Conversation/)).getByLabelText("1 unread message").textContent).toBe("1")

  await arrives(queryClient, said, "01JMESG5")
  expect(within(tab(/Conversation/)).getByLabelText("2 unread messages").textContent).toBe("2")
})

/**
 * A `Cancel task` alone on a line under the title read as a second row of
 * header, and which line it landed on came down to how long the title was and
 * how many buttons the status offers — an in-progress task offers one, a
 * pending one two. The title gives way instead, in both.
 */
it("keeps the actions on the title row whatever the status offers", () => {
  mount()

  const cancel = screen.getByRole("button", { name: "Cancel task" })
  const title = screen.getByRole("heading", { name: TASK.title })
  expect(title.parentElement?.contains(cancel)).toBe(true)
})

it("folds the sessions table down to what a panel holds", async () => {
  stubSessions()
  mount()
  await userEvent.setup().click(screen.getByRole("tab", { name: /^Sessions/ }))

  const open = await screen.findByRole("button", { name: "Open Engineer session" })
  const row = open.closest("tr")
  if (!row) throw new Error("no row around the session")

  // Four cells, not the screen's seven: the id shares the role's cell rather
  // than taking one of its own, and the tokens have none at all.
  const cells = within(row).getAllByRole("cell")
  expect(cells).toHaveLength(4)
  const session = cells[0]
  if (!session) throw new Error("no session cell in the row")
  expect(session.textContent).toContain("Engineer")
  expect(session.textContent).toContain(shortId(SESSION.id))

  // And it shares it on one line: a single flex row holds the role and the id
  // both, with nothing block-level between them to push the id underneath.
  const line = session.firstElementChild
  const id = within(session).getByText(shortId(SESSION.id))
  expect(line?.className).toContain("flex")
  expect(line?.contains(open)).toBe(true)
  expect(line?.contains(id)).toBe(true)
  for (const wrapper of [open.parentElement, id.parentElement]) {
    expect(wrapper?.className).not.toContain("block")
  }

  // What it spent is behind the last activity, with the two stamps that were
  // already there — the figure is the plain pair, since a hint cannot hold a
  // hint of its own.
  const age = cells[3]?.querySelector<HTMLElement>("[data-slot='tooltip-trigger']")
  if (!age) throw new Error("no last-activity hint in the row")
  age.focus()
  const popup = await waitFor(() => {
    const hint = document.querySelector<HTMLElement>("[data-slot='tooltip-content']")
    if (!hint) throw new Error("no hint behind the last activity")
    return hint
  })
  expect(popup.textContent).toContain("started")
  expect(popup.textContent).toContain("1M in, 90% cached, 40k out")
})

/**
 * The way back out of a session picked inside the panel. A button is
 * `whitespace-nowrap`, so a title long enough runs it straight under the
 * sheet's close button; the label truncates instead, and the link stays one
 * line at every width.
 */
it("keeps the way back from a session to one line", async () => {
  stubSessions()
  mount()
  const user = userEvent.setup()
  await user.click(screen.getByRole("tab", { name: /^Sessions/ }))
  await user.click(await screen.findByRole("button", { name: "Open Engineer session" }))

  const back = await screen.findByRole("button", { name: `Back to ${TASK.title}` })
  expect(back.className).toContain("max-w-full")
  expect(back.querySelector("span")?.className).toContain("truncate")
})

/**
 * How much is behind each tab, on the tab itself: a task lands on its
 * description, and finding out whether anything has reviewed it or run it
 * meant opening the two tabs to see. The numbers come off the very cache
 * entries those tabs read, so a count costs no request the panel was not
 * already making.
 */
it("says how many sessions, reviews and messages are behind the tabs", async () => {
  stubTabLists()
  markThreadSeen(`task:${TASK.id}`, "01JMESG1")
  mount(TASK, THREAD)

  // Named for what they count, so a screen reader hears "Sessions, 1 session"
  // rather than "Sessions 1".
  expect((await within(tab(/^Sessions/)).findByLabelText("1 session")).textContent).toBe("1")
  expect((await within(tab(/^Reviews/)).findByLabelText("2 reviews")).textContent).toBe("2")

  // The conversation carries two numbers, and they answer different questions:
  // the muted total of what has been said, then the filled count of what the
  // reader has not seen.
  const conversation = within(tab(/^Conversation/))
  expect(conversation.getByLabelText("3 messages").textContent).toBe("3")
  expect(conversation.getByLabelText("2 unread messages").textContent).toBe("2")
  expect(tab(/^Conversation/).textContent).toBe("Conversation32")

  // The other three have no number to carry: a description is one thing, and
  // a diff and a transition log are not lists the reader is counting.
  expect(tab("Description").textContent).toBe("Description")
  expect(tab("History").textContent).toBe("History")
  expect(tab("Diff").textContent).toBe("Diff")
})

it("waits for a list before putting a number on its tab, then says zero", async () => {
  stubTabLists([], [])
  mount()

  // The first render is the request going out, so neither tab has an answer
  // yet: a count that starts at zero and jumps would say the task has never
  // run for as long as the request takes.
  expect(tab("Sessions").textContent).toBe("Sessions")
  expect(tab("Reviews").textContent).toBe("Reviews")

  // Zero itself is shown — it is an answer, and the one the tab would have
  // been opened to find.
  await waitFor(() => expect(tab(/^Sessions/).textContent).toBe("Sessions0"))
  expect(within(tab(/^Reviews/)).getByLabelText("0 reviews").textContent).toBe("0")
})
