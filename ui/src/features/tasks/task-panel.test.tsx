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
 * the two reviewer slots that share nothing but their order.
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
 * seat's own line, and what the session spent behind its last activity.
 *
 * Everything is seeded into the query cache: what the daemon returns is
 * `queries.ts`'s story, and the tabs are `task-panel.tsx`'s own.
 */

import { screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { expect, it } from "vitest"

import { type MessageDto, qk, type SessionDto, type TaskDto, type TaskPickDto } from "@/api"
import { shortId } from "@/lib/format"
import { aSession } from "@/test/fixtures"
import { daemonFetch, jsonResponse, renderScreen } from "@/test/harness"
import { TaskPanel } from "./task-panel"

const TASK: TaskDto = {
  id: "01JTASK0000000000000000001",
  goal_id: "01JGOAL0000000000000000001",
  repo_id: "01JREPO0000000000000000001",
  title: "Surface the pins",
  description: "",
  status: "in_progress",
  branch: "surface-the-pins-000001",
  landing: "merge",
  depends_on: [],
  agents: [
    {
      id: "01AGENTAUTHOR",
      seat: "author",
      skills: ["coding"],
      model: "codex:gpt-5",
      effort: "xhigh",
    },
    {
      id: "01AGENTSTRICT",
      seat: "reviewer",
      skills: ["code-review"],
      model: "claude_code:claude-sonnet-5",
      effort: "high",
    },
    {
      id: "01AGENTSTRICT2",
      seat: "reviewer",
      skills: ["security-review"],
      model: "codex:gpt-5.6-luna",
    },
  ],
  picks: [],
  stalled: false,
  usage: {
    total: { input_tokens: 1_234_567, cached_input_tokens: 1_100_000, output_tokens: 45_300 },
    author: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
    // Only the reviewer that has actually been spawned: the second slot has
    // never run, and the daemon lists no row for it.
    reviewers: [
      {
        agent_id: "01AGENTSTRICT",
        skills: ["code-review"],
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
  task_agent_id: "01AGENTAUTHOR",
  usage: { input_tokens: 1_000_000, cached_input_tokens: 900_000, output_tokens: 40_000 },
})

function mount(task: TaskDto = TASK) {
  return renderScreen(<TaskPanel taskId={task.id} onClose={() => {}} />, {
    seed: (client) => {
      client.setQueryData(qk.tasks.detail(task.id), task)
    },
  })
}

/** One of the panel's tabs, by name. */
function tab(name: RegExp | string): HTMLElement {
  return screen.getByRole("tab", { name })
}

/** Two verdicts in one round: what the Messages tab lists, and so what it counts. */
const MESSAGES: MessageDto[] = ["01AGENTSTRICT", "01AGENTAUTO"].map((reviewer, index) => ({
  id: `01JMSGS00000000000000000${index}`,
  goal_id: TASK.goal_id,
  task_id: TASK.id,
  round: 0,
  kind: "approve",
  from_actor: "reviewer",
  from_agent_id: reviewer,
  to_actor: "author",
  to_agent_id: "01AGENTAUTHOR",
  body: "looks right",
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
function stubTabLists(sessions: SessionDto[] = [SESSION], messages: MessageDto[] = MESSAGES) {
  daemonFetch.mockImplementation((input: Request | string | URL) => {
    const url = new URL(typeof input === "string" ? input : (input as Request).url)
    if (url.pathname === "/v1/sessions") return Promise.resolve(jsonResponse(sessions))
    if (url.pathname === `/v1/tasks/${TASK.id}/messages`) {
      return Promise.resolve(jsonResponse(messages))
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

it("shows the author's pin as it was staffed", () => {
  mount()

  expect(fact("Author")).toContain("coding")
  // The model and, after an `@`, the effort it is run at: one pin, one line.
  expect(fact("Author")).toContain("codex:gpt-5 @ xhigh")
})

it("leaves the effort off a pin that names none, which is the CLI's own", () => {
  mount({
    ...TASK,
    agents: [{ id: "01AGENTAUTHOR", seat: "author", skills: ["coding"], model: "codex:gpt-5" }],
  })

  expect(fact("Author")).toContain("codex:gpt-5")
  expect(fact("Author")).not.toContain("@")
})

it("shows each reviewer slot's own pin, in review order", () => {
  mount()

  const reviewers = fact("Reviewers")
  expect(reviewers).toContain("code-review · claude_code:claude-sonnet-5 @ high")
  expect(reviewers).toContain("security-review · codex:gpt-5.6-luna")
})

it("says a task has no reviewers rather than showing an empty list", () => {
  mount({
    ...TASK,
    agents: [
      {
        id: "01AGENTAUTHOR",
        seat: "author",
        skills: ["coding"],
        model: "codex:gpt-5",
      },
    ],
  })

  expect(fact("Reviewers")).toBe("none staffed")
})

it("keeps the singular Author fact and shows no pick on a one-author task", () => {
  mount()

  expect(screen.queryByText("Authors")).toBeNull()
  expect(screen.queryByText("Picks")).toBeNull()
})

/** Both reviewers picking the second author, oldest first. */
const FIRST_PICK: TaskPickDto = {
  reviewer_agent_id: "01AGENTSTRICT",
  author_agent_id: "01AGENTAUTHOR2",
  created_at: "2026-01-01T00:00:00Z",
}
const SECOND_PICK: TaskPickDto = {
  reviewer_agent_id: "01AGENTSTRICT2",
  author_agent_id: "01AGENTAUTHOR2",
  created_at: "2026-01-01T00:00:01Z",
}

/** A task staffed with two authors, and its pick: both reviewers picked the second. */
const TWO_AUTHOR_TASK: TaskDto = {
  ...TASK,
  agents: [
    {
      id: "01AGENTAUTHOR",
      seat: "author",
      skills: ["coding"],
      model: "codex:gpt-5",
      effort: "xhigh",
      branch: "surface-the-pins-000001",
    },
    {
      id: "01AGENTAUTHOR2",
      seat: "author",
      skills: ["testing"],
      model: "claude_code:claude-sonnet-5",
      branch: "surface-the-pins-000001-b",
    },
    ...TASK.agents.filter((agent) => agent.seat === "reviewer"),
  ],
  picked_agent_id: "01AGENTAUTHOR2",
  picks: [FIRST_PICK, SECOND_PICK],
}

it("shows every author's own branch, marking only the one the reviewers picked", () => {
  mount(TWO_AUTHOR_TASK)

  const authors = fact("Authors")
  expect(authors).toContain("coding · codex:gpt-5 @ xhigh")
  expect(authors).toContain("testing · claude_code:claude-sonnet-5")
  expect(authors).toContain("surface-the-pins-000001")
  expect(authors).toContain("surface-the-pins-000001-b")

  // Exactly one "Picked" mark, on the winner's own line.
  const picked = screen.getByText("Picked")
  const row = picked.closest(".flex-wrap")?.textContent ?? ""
  expect(row).toContain("surface-the-pins-000001-b")
  expect(row).not.toContain("codex:gpt-5")

  // The author nobody picked says so, rather than showing nothing at all —
  // every author gets its own status, not just the one that won. It reads
  // "0 picks" and not the unsettled "no picks yet": the pick has settled, and
  // this author simply got none of it.
  expect(authors).toContain("0 picks")
})

it("shows an author's own vote count before the pick settles", () => {
  mount({ ...TWO_AUTHOR_TASK, picked_agent_id: null, picks: [FIRST_PICK] })

  const authors = fact("Authors")
  expect(authors).toContain("1 pick")
  expect(authors).toContain("0 picks")
  expect(screen.queryByText("Picked")).toBeNull()
})

it("lists what each reviewer picked, oldest first", () => {
  mount(TWO_AUTHOR_TASK)

  const picks = fact("Picks")
  expect(picks).toContain("code-review picked testing")
  expect(picks).toContain("security-review picked testing")
})

it("says a several-author task has no picks yet, rather than showing an empty list", () => {
  mount({ ...TWO_AUTHOR_TASK, picked_agent_id: null, picks: [] })

  expect(fact("Picks")).toBe("no picks yet")
  // Every author echoes it too, rather than a settled-sounding "0 picks"
  // before the pick has even started.
  expect(fact("Authors")).not.toContain("0 picks")
  const authorCount = (fact("Authors").match(/no picks yet/g) ?? []).length
  expect(authorCount).toBe(2)
  expect(screen.queryByText("Picked")).toBeNull()
})

it("links the pull request its author published", () => {
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
      author: { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
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

  // The author first, then the one reviewer that has actually run — named
  // by the daemon, since the figures are its own. The second slot has never
  // been spawned and is not a line at all.
  const who = [...popup.querySelectorAll("dt")].map((agent) => agent.textContent)
  expect(who).toEqual(["Author", "code-review"])
  expect(popup.textContent).not.toContain("security-review")

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

  const open = await screen.findByRole("button", { name: "Open Author session" })
  const row = open.closest("tr")
  if (!row) throw new Error("no row around the session")

  // Four cells, not the screen's seven: the id shares the seat's cell rather
  // than taking one of its own, and the tokens have none at all.
  const cells = within(row).getAllByRole("cell")
  expect(cells).toHaveLength(4)
  const session = cells[0]
  if (!session) throw new Error("no session cell in the row")
  expect(session.textContent).toContain("Author")
  expect(session.textContent).toContain(shortId(SESSION.id))

  // And it shares it on one line: a single flex row holds the seat and the id
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
  await user.click(await screen.findByRole("button", { name: "Open Author session" }))

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
it("says how many sessions and messages are behind the tabs", async () => {
  stubTabLists()
  mount()

  // Named for what they count, so a screen reader hears "Sessions, 1 session"
  // rather than "Sessions 1".
  expect((await within(tab(/^Sessions/)).findByLabelText("1 session")).textContent).toBe("1")
  expect((await within(tab(/^Messages/)).findByLabelText("2 messages")).textContent).toBe("2")

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
  expect(tab("Messages").textContent).toBe("Messages")

  // Zero itself is shown — it is an answer, and the one the tab would have
  // been opened to find.
  await waitFor(() => expect(tab(/^Sessions/).textContent).toBe("Sessions0"))
  expect(within(tab(/^Messages/)).getByLabelText("0 messages").textContent).toBe("0")
})
