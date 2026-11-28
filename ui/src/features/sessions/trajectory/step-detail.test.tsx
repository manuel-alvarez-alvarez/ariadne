// @vitest-environment jsdom

/**
 * A step of the turn console opened in place, and in the full view, driven
 * from the daemon's side: the trajectory stream on an `EventSource`
 * stand-in, and the detail, steps and cancel routes on the fetch stub.
 */

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import type { ContentBlockDto, StepDto, TurnDto } from "@/api"
import { latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, renderScreen } from "@/test/harness"
import { anUpdate, aSnapshot, aSubagentStep, aToolStep, aTurn } from "@/test/trajectory"

import { TrajectoryConsole } from "./trajectory-console"

const SESSION_ID = "01JSESSDETAIL0000000000001"
const DAEMON = "http://127.0.0.1:7676"

type ToolStep = Extract<StepDto, { type: "tool_call" }>

/** Each request the console made, with its path, query and body. */
const requests: { method: string; path: string; query: URLSearchParams; body: string }[] = []
/** The typed detail of each tool call step, by step id. */
let details: Record<string, { detail: unknown; raw?: { input: unknown; output: unknown } }> = {}
/** The steps of each subagent, by subagent id. */
let subagentSteps: Record<string, StepDto[]> = {}
/** How many steps of a subagent one page of the steps route holds. */
let pageSize = 200
let resize: (width: number) => void

beforeEach(() => {
  stubEventSource()
  requests.length = 0
  details = {}
  subagentSteps = {}
  pageSize = 200
  resize = () => {}
  daemonFetch.mockImplementation(async (input: Request | string | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init)
    const url = new URL(request.url)
    requests.push({
      method: request.method,
      path: url.pathname,
      query: url.searchParams,
      body: await request.text(),
    })
    const item = url.pathname.match(/\/trajectory\/items\/([^/]+)$/)
    if (item?.[1]) {
      const held = details[item[1]]
      if (!held) return new Response(null, { status: 404 })
      return Response.json({
        id: item[1],
        tool_call_id: "call",
        title: "call",
        status: "completed",
        detail_rev: 1,
        detail: held.detail,
        ...(url.searchParams.get("raw") === "true" ? { raw: held.raw } : {}),
      })
    }
    const subagent = url.searchParams.get("subagent")
    if (url.pathname.endsWith("/steps")) {
      // The newest page before `before`, as the daemon pages a lane.
      const before = url.searchParams.get("before")
      const all = (subagent ? (subagentSteps[subagent] ?? []) : []).filter(
        (step) => before === null || step.id < before,
      )
      const page = all.slice(-pageSize)
      return Response.json({ steps: page, earlier_steps: all.length - page.length })
    }
    if (url.pathname.endsWith("/questions")) return Response.json([])
    return new Response(null, { status: 204 })
  })
})

/** Measure the console panel as `width` CSS pixels wide. */
function stubPanelWidth() {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      callback: ResizeObserverCallback
      constructor(callback: ResizeObserverCallback) {
        this.callback = callback
      }
      observe(target: Element) {
        if (target.hasAttribute("data-step-workspace"))
          resize = (width) =>
            this.callback(
              [{ target, contentRect: { width } } as ResizeObserverEntry],
              this as unknown as ResizeObserver,
            )
      }
      unobserve() {}
      disconnect() {}
    },
  )
}

/** A running turn whose steps show live under its activity line. */
function renderTurn(
  steps: (turnId: string) => StepDto[],
  options: { status?: "running" | "idle"; answer?: ContentBlockDto[] } = {},
) {
  const base = aTurn({ state: "running", ended_at: null })
  const turn: TurnDto = {
    ...base,
    steps: steps(base.id),
    ...(options.answer ? { state: "completed", answer: options.answer } : {}),
  }
  renderScreen(
    <TrajectoryConsole host="local" sessionId={SESSION_ID} status={options.status ?? "idle"} />,
  )
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", aSnapshot([turn]))
  })
  return { source, turn }
}

function transcript() {
  return screen.getByRole("log", { name: "Console transcript" })
}

function row(step: StepDto): HTMLElement {
  const found = transcript().querySelector<HTMLElement>(`button[data-step-id="${step.id}"]`)
  if (!found) throw new Error(`step ${step.id} has no row`)
  return found
}

function detailOf(step: StepDto): HTMLElement | null {
  return transcript().querySelector<HTMLElement>(`[data-step-detail="${step.id}"]`)
}

function section(root: HTMLElement, name: string): HTMLElement {
  const found = root.querySelector<HTMLElement>(`[data-section="${name}"]`)
  if (!found) throw new Error(`no section ${name}`)
  return found
}

function itemReads(step: StepDto) {
  return requests.filter((one) => one.path.endsWith(`/trajectory/items/${step.id}`))
}

it("opens a step under its row on a click, Enter or Space, to a set height, and closes it the same way", async () => {
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId)
    return [tool]
  })
  details[tool.id] = { detail: { type: "command", command: "npm test", truncated: false } }

  expect(row(tool).getAttribute("aria-expanded")).toBe("false")
  expect(detailOf(tool)).toBeNull()

  await user.click(row(tool))
  expect(row(tool).getAttribute("aria-expanded")).toBe("true")
  expect(detailOf(tool)?.className).toContain("max-h-80")
  await user.click(row(tool))
  expect(detailOf(tool)).toBeNull()

  row(tool).focus()
  await user.keyboard("{Enter}")
  expect(detailOf(tool)).not.toBeNull()
  await user.keyboard("{Enter}")
  expect(detailOf(tool)).toBeNull()
  await user.keyboard(" ")
  expect(detailOf(tool)).not.toBeNull()
  await user.keyboard(" ")
  expect(detailOf(tool)).toBeNull()
})

it("draws a command as its command and its output", async () => {
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId)
    return [tool]
  })
  details[tool.id] = {
    detail: {
      type: "command",
      command: "npm test -- --run",
      cwd: "/work/ui",
      output: "Tests 12 passed",
      exit_code: 0,
      truncated: false,
    },
  }

  fireEvent.click(row(tool))
  const detail = detailOf(tool) as HTMLElement
  await waitFor(() =>
    expect(section(detail, "command").querySelector("code")?.textContent).toBe("npm test -- --run"),
  )
  within(section(detail, "output")).getByText("Tests 12 passed")
  within(detail).getByText("in /work/ui")
  within(detail).getByText("exit code 0")
})

it("draws an edit as a diff with line numbers and language colours", async () => {
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId, { kind: "edit", title: "Edit src/app.ts" })
    return [tool]
  })
  details[tool.id] = {
    detail: {
      type: "diff",
      files: [
        {
          path: "src/app.ts",
          language: "typescript",
          old_text: null,
          new_text: null,
          unified:
            "--- src/app.ts\n+++ src/app.ts\n@@ -10,2 +10,2 @@\n const keep = 1\n-const gone = 2\n+const added = 3\n",
        },
      ],
    },
  }

  fireEvent.click(row(tool))
  const file = await waitFor(() => section(detailOf(tool) as HTMLElement, "src/app.ts"))
  const numbers = await waitFor(() => {
    const gutter = file.querySelector(".cm-newLineNumbers")
    if (!gutter) throw new Error("no line numbers yet")
    return gutter
  })
  expect(
    [...numbers.querySelectorAll<HTMLElement>(".cm-gutterElement")]
      .filter((element) => element.style.visibility !== "hidden")
      .map((element) => element.textContent),
  ).toEqual(["10", "11"])
  // The language's grammar colours the code: a keyword is a classed run of its own.
  const keyword = [...file.querySelectorAll<HTMLElement>(".cm-line span[class]")].find(
    (span) => span.textContent === "const",
  )
  expect(keyword).toBeDefined()
  within(file).getByText("TypeScript")
})

it("draws a read as the file with its language and its line numbers", async () => {
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId, { kind: "read", title: "Read src/main.rs" })
    return [tool]
  })
  details[tool.id] = {
    detail: {
      type: "file",
      path: "src/main.rs",
      line: 12,
      language: "rust",
      text: "fn main() {\n}\n",
    },
  }

  fireEvent.click(row(tool))
  const file = await waitFor(() => section(detailOf(tool) as HTMLElement, "src/main.rs"))
  expect(
    [...file.querySelectorAll<HTMLElement>("[data-line]")].map((line) => line.dataset.line),
  ).toEqual(["12", "13"])
  expect(file.querySelector("[data-code-language]")?.textContent).toBe("Rust")
})

it("draws a search and a fetch as the query or the URL, and the result", async () => {
  let search = {} as ToolStep
  let fetchStep = {} as ToolStep
  renderTurn((turnId) => {
    search = aToolStep(turnId, { kind: "search", title: "Grep flaky" })
    fetchStep = aToolStep(turnId, { kind: "fetch", title: "Fetch the docs" })
    return [search, fetchStep]
  })
  details[search.id] = {
    detail: { type: "search", query: "flaky", text: "src/a.test.ts:3" },
  }
  details[fetchStep.id] = {
    detail: { type: "fetch", url: "https://example.com/docs", text: "The docs" },
  }

  fireEvent.click(row(search))
  fireEvent.click(row(fetchStep))
  const found = detailOf(search) as HTMLElement
  await waitFor(() => within(section(found, "query")).getByText("flaky"))
  within(section(found, "result")).getByText("src/a.test.ts:3")
  const fetched = detailOf(fetchStep) as HTMLElement
  await waitFor(() => within(section(fetched, "url")).getByText("https://example.com/docs"))
  within(section(fetched, "result")).getByText("The docs")
})

it("shows the raw input and the raw output only behind Raw", async () => {
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId)
    return [tool]
  })
  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "ok", truncated: false },
    raw: { input: { command: "secret-input" }, output: { stdout: "secret-output" } },
  }

  await user.click(row(tool))
  const detail = detailOf(tool) as HTMLElement
  await within(detail).findByText("npm test")
  expect(itemReads(tool).map((one) => one.query.get("raw"))).toEqual([null])
  expect(within(detail).queryByText(/secret-/)).toBeNull()

  const raw = within(detail).getByRole("button", { name: "Raw" })
  expect(raw.textContent).toBe("")
  await user.click(raw)
  expect(raw.getAttribute("aria-pressed")).toBe("true")
  await within(section(detail, "raw input")).findByText(/secret-input/)
  within(section(detail, "raw output")).getByText(/secret-output/)
  expect(itemReads(tool).at(-1)?.query.get("raw")).toBe("true")

  await user.click(raw)
  expect(within(detail).queryByText(/secret-/)).toBeNull()
})

it("copies the content of a section", async () => {
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId)
    return [tool]
  })
  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "Tests 12 passed", truncated: false },
  }

  await user.click(row(tool))
  const output = await waitFor(() => section(detailOf(tool) as HTMLElement, "output"))
  await user.click(within(output).getByRole("button", { name: "Copy output" }))
  expect(await navigator.clipboard.readText()).toBe("Tests 12 passed")
})

it("shows an image inline in a message and in a detail, and audio and resources as labelled rows with a link", async () => {
  const blob = `/v1/sessions/${SESSION_ID}/trajectory/blobs/01EVENT/1`
  let tool = {} as ToolStep
  renderTurn(
    (turnId) => {
      tool = aToolStep(turnId, { kind: "other", title: "Take a screenshot" })
      return [tool]
    },
    {
      answer: [
        { type: "text", text: "Here it is" },
        { type: "image", blob, mime_type: "image/png" },
      ],
    },
  )
  details[tool.id] = {
    detail: {
      type: "generic",
      content: [
        { type: "image", blob: `${blob}0`, mime_type: "image/png" },
        { type: "audio", blob: `${blob}1`, mime_type: "audio/wav" },
        { type: "resource", uri: "file:///notes.md", blob: `${blob}2` },
        { type: "resource_link", uri: "https://example.com/spec", name: "spec" },
      ],
    },
  }

  const images = () =>
    within(transcript())
      .getAllByRole("img")
      .filter((image) => image.tagName === "IMG")
      .map((image) => image.getAttribute("src"))
  expect(images()).toEqual([`${DAEMON}${blob}`])

  // The turn ended, so its fold is closed: open it to the step row.
  requests.length = 0
  fireEvent.click(within(transcript()).getByRole("button", { name: /1 failed/ }))
  await waitFor(() => row(tool))
  fireEvent.click(row(tool))
  const detail = detailOf(tool) as HTMLElement
  // The step's detail comes before the answer of its turn.
  await waitFor(() => expect(images()).toEqual([`${DAEMON}${blob}0`, `${DAEMON}${blob}`]))
  const link = (label: string) =>
    within(detail.querySelector<HTMLElement>(`[data-content-row="${label}"]`) as HTMLElement)
      .getByRole("link")
      .getAttribute("href")
  expect(link("Audio")).toBe(`${DAEMON}${blob}1`)
  expect(link("Resource")).toBe(`${DAEMON}${blob}2`)
  expect(link("Resource link")).toBe("https://example.com/spec")
})

it("reads the detail of an open running call again at each new detail_rev", async () => {
  let tool = {} as ToolStep
  const { source } = renderTurn((turnId) => {
    tool = aToolStep(turnId, { status: "running", ended_at: null })
    return [tool]
  })
  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "first line", truncated: false },
  }
  fireEvent.click(row(tool))
  const detail = detailOf(tool) as HTMLElement
  await within(detail).findByText("first line")

  details[tool.id] = {
    detail: {
      type: "command",
      command: "npm test",
      output: "first line\nsecond line",
      truncated: false,
    },
  }
  act(() =>
    source.emit("update", anUpdate(2, [{ op: "item_upsert", step: { ...tool, detail_rev: 2 } }])),
  )
  await within(section(detail, "output")).findByText(/second line/)
  expect(itemReads(tool)).toHaveLength(2)
})

it("draws a subagent step with its title, task, state, time and step count, opens it to its steps, and cancels it", async () => {
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  let inner = {} as ToolStep
  renderTurn((turnId) => {
    subagent = aSubagentStep(turnId)
    return [subagent]
  })
  inner = aToolStep(subagent.turn_id, { lane: subagent.subagent_id, title: "Run one test" })
  subagentSteps[subagent.subagent_id] = [inner]
  details[inner.id] = {
    detail: { type: "command", command: "npm test one", output: "passed", truncated: false },
  }

  const line = row(subagent)
  for (const fact of ["Explore the tests", "Find the flaky test", "4 steps"])
    within(line).getByText(fact)
  for (const word of ["subagent", "running"]) within(line).getByRole("img", { name: word })
  expect(line.textContent).toMatch(/\d+m \d+s|\d+s/)

  await user.click(line)
  const detail = detailOf(subagent) as HTMLElement
  const steps = await within(detail).findByRole("list", { name: "Steps of Explore the tests" })
  const read = requests.find((one) => one.query.get("subagent") === subagent.subagent_id)
  expect(read?.path).toBe(`/v1/sessions/${SESSION_ID}/trajectory/turns/${subagent.turn_id}/steps`)
  // A step of the subagent opens in place too.
  await user.click(within(steps).getByRole("button", { name: /Run one test/ }))
  await within(steps).findByText("passed")

  await user.click(
    within(transcript()).getByRole("button", { name: "Cancel the subagent Explore the tests" }),
  )
  await waitFor(() =>
    expect(
      requests
        .filter((one) => one.method === "POST" && one.path.endsWith("/console/cancel"))
        .map((one) => JSON.parse(one.body)),
    ).toEqual([{ subagent_id: subagent.subagent_id }]),
  )
})

it("offers no cancel on a subagent the agent does not let cancel", () => {
  let subagent = aSubagentStep("none")
  renderTurn((turnId) => {
    subagent = aSubagentStep(turnId, { can_cancel: false, state: "completed" })
    return [subagent]
  })
  within(row(subagent)).getByRole("img", { name: "completed" })
  expect(within(transcript()).queryByRole("button", { name: /Cancel the subagent/ })).toBeNull()
})

it("opens the whole step beside the conversation on a panel of 880 px or wider", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  renderTurn((turnId) => {
    subagent = aSubagentStep(turnId)
    return [subagent]
  })
  subagentSteps[subagent.subagent_id] = [
    aToolStep(subagent.turn_id, { lane: subagent.subagent_id, title: "Run one test" }),
  ]
  act(() => resize(1000))

  await user.click(row(subagent))
  await user.click(within(detailOf(subagent) as HTMLElement).getByRole("button", { name: "Open" }))
  const view = screen.getByRole("complementary", { name: "Full view" })
  expect(transcript().contains(view)).toBe(false)
  // The subagent's whole work: its steps, in the full view.
  await within(view).findByRole("button", { name: /Run one test/ })
  expect(screen.queryByRole("dialog")).toBeNull()
})

it("opens the whole step in a drawer on a panel narrower than 880 px", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn((turnId) => {
    tool = aToolStep(turnId)
    return [tool]
  })
  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "the whole output", truncated: false },
  }
  act(() => resize(600))

  await user.click(row(tool))
  const detail = detailOf(tool) as HTMLElement
  await within(detail).findByText("the whole output")
  await user.click(within(detail).getByRole("button", { name: "Open" }))
  const drawer = await screen.findByRole("dialog", { name: "Run the tests" })
  await within(drawer).findByText("the whole output")
  expect(within(drawer).queryByRole("button", { name: "Open" })).toBeNull()
  expect(screen.queryByRole("complementary", { name: "Full view" })).toBeNull()
})

it("closes the full view on Escape and does not cancel the running turn", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn(
    (turnId) => {
      tool = aToolStep(turnId, { status: "running", ended_at: null })
      return [tool]
    },
    { status: "running" },
  )
  details[tool.id] = { detail: { type: "command", command: "npm test", truncated: false } }
  act(() => resize(1000))

  await user.click(row(tool))
  const open = within(detailOf(tool) as HTMLElement).getByRole("button", { name: "Open" })
  await user.click(open)
  const view = screen.getByRole("complementary", { name: "Full view" })
  await waitFor(() =>
    expect(document.activeElement).toBe(
      within(view).getByRole("button", { name: "Close the full view" }),
    ),
  )

  await user.keyboard("{Escape}")
  expect(screen.queryByRole("complementary", { name: "Full view" })).toBeNull()
  expect(document.activeElement).toBe(open)
  expect(requests.filter((one) => one.path.endsWith("/console/cancel"))).toEqual([])

  // Escape elsewhere in the console still cancels the turn.
  await user.keyboard("{Escape}")
  await waitFor(() =>
    expect(requests.filter((one) => one.path.endsWith("/console/cancel"))).toHaveLength(1),
  )
})

it("shows the newest progress of a running call in the full view", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let tool = {} as ToolStep
  const { source } = renderTurn((turnId) => {
    tool = aToolStep(turnId, { status: "running", ended_at: null })
    return [tool]
  })
  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "first line", truncated: false },
  }
  act(() => resize(1000))
  await user.click(row(tool))
  await user.click(within(detailOf(tool) as HTMLElement).getByRole("button", { name: "Open" }))
  const view = screen.getByRole("complementary", { name: "Full view" })
  await within(view).findByText("first line")

  details[tool.id] = {
    detail: { type: "command", command: "npm test", output: "second line", truncated: false },
  }
  act(() =>
    source.emit("update", anUpdate(2, [{ op: "item_upsert", step: { ...tool, detail_rev: 2 } }])),
  )
  await within(view).findByText("second line")
})

function rawToggle(root: HTMLElement): HTMLElement {
  return within(root).getByRole("button", { name: "Raw" })
}

it("starts each step the full view shows afresh, with Raw off", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let first = {} as ToolStep
  let second = {} as ToolStep
  renderTurn((turnId) => {
    first = aToolStep(turnId, { title: "First call" })
    second = aToolStep(turnId, { title: "Second call" })
    return [first, second]
  })
  details[first.id] = {
    detail: { type: "command", command: "first", truncated: false },
    raw: { input: { from: "first-raw" }, output: null },
  }
  details[second.id] = {
    detail: { type: "command", command: "second", truncated: false },
    raw: { input: { from: "second-raw" }, output: null },
  }
  act(() => resize(1000))

  await user.click(row(first))
  await user.click(within(detailOf(first) as HTMLElement).getByRole("button", { name: "Open" }))
  const view = screen.getByRole("complementary", { name: "Full view" })
  await user.click(rawToggle(view))
  await within(view).findByText(/first-raw/)

  await user.click(row(second))
  await user.click(within(detailOf(second) as HTMLElement).getByRole("button", { name: "Open" }))
  await waitFor(() =>
    expect(section(view, "command").querySelector("code")?.textContent).toBe("second"),
  )
  expect(rawToggle(view).getAttribute("aria-pressed")).toBe("false")
  expect(within(view).queryByText(/-raw/)).toBeNull()
})

it("shows the new progress of a subagent's running call while its step count holds", async () => {
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  const { source } = renderTurn((turnId) => {
    subagent = aSubagentStep(turnId)
    return [subagent]
  })
  const inner = aToolStep(subagent.turn_id, {
    lane: subagent.subagent_id,
    title: "Run one test",
    status: "running",
    ended_at: null,
  })
  subagentSteps[subagent.subagent_id] = [inner]
  details[inner.id] = {
    detail: { type: "command", command: "npm test one", output: "first line", truncated: false },
  }

  await user.click(row(subagent))
  const steps = await within(detailOf(subagent) as HTMLElement).findByRole("list", {
    name: "Steps of Explore the tests",
  })
  await user.click(within(steps).getByRole("button", { name: /Run one test/ }))
  await within(steps).findByText("first line")

  details[inner.id] = {
    detail: { type: "command", command: "npm test one", output: "second line", truncated: false },
  }
  act(() =>
    source.emit("update", anUpdate(2, [{ op: "item_upsert", step: { ...inner, detail_rev: 2 } }])),
  )
  await within(steps).findByText("second line")
  // The subagent's own count did not move, so its steps were read once.
  expect(requests.filter((one) => one.query.get("subagent") === subagent.subagent_id)).toHaveLength(
    1,
  )
})

it("reads the earlier steps of a subagent with Show N earlier", async () => {
  const user = userEvent.setup()
  pageSize = 2
  let subagent = aSubagentStep("none")
  renderTurn((turnId) => {
    subagent = aSubagentStep(turnId, { activity: { ...aSubagentStep(turnId).activity, steps: 3 } })
    return [subagent]
  })
  const [oldest, middle, newest] = ["Oldest call", "Middle call", "Newest call"].map((title) =>
    aToolStep(subagent.turn_id, { lane: subagent.subagent_id, title }),
  ) as [ToolStep, ToolStep, ToolStep]
  subagentSteps[subagent.subagent_id] = [oldest, middle, newest]

  await user.click(row(subagent))
  const detail = detailOf(subagent) as HTMLElement
  const steps = await within(detail).findByRole("list", { name: "Steps of Explore the tests" })
  expect(within(steps).queryByRole("button", { name: /Oldest call/ })).toBeNull()

  await user.click(within(detail).getByRole("button", { name: "Show 1 earlier" }))
  await within(steps).findByRole("button", { name: /Oldest call/ })
  expect(
    within(steps)
      .getAllByRole("button")
      .map((button) => button.textContent?.match(/(?:Oldest|Middle|Newest) call/)?.[0]),
  ).toEqual(["Oldest call", "Middle call", "Newest call"])
  expect(requests.at(-1)?.query.get("before")).toBe(middle.id)
  expect(within(detail).queryByRole("button", { name: /earlier/ })).toBeNull()
})

it("keeps the keyboard in the full view when an Open inside it shows another step, and gives it back to the first Open", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  renderTurn(
    (turnId) => {
      subagent = aSubagentStep(turnId)
      return [subagent]
    },
    { status: "running" },
  )
  const inner = aToolStep(subagent.turn_id, { lane: subagent.subagent_id, title: "Run one test" })
  subagentSteps[subagent.subagent_id] = [inner]
  details[inner.id] = { detail: { type: "command", command: "npm test one", truncated: false } }
  act(() => resize(1000))

  row(subagent).focus()
  await user.keyboard("{Enter}")
  const open = within(detailOf(subagent) as HTMLElement).getByRole("button", { name: "Open" })
  open.focus()
  await user.keyboard("{Enter}")
  const view = screen.getByRole("complementary", { name: "Full view" })
  const close = within(view).getByRole("button", { name: "Close the full view" })
  await waitFor(() => expect(document.activeElement).toBe(close))

  // Down the subagent's work by keyboard, to the Open of one of its steps.
  const innerRow = await within(view).findByRole("button", { name: /Run one test/ })
  innerRow.focus()
  await user.keyboard("{Enter}")
  const innerOpen = await waitFor(() => {
    const found = within(innerRow.closest("li") as HTMLElement).getAllByRole("button", {
      name: "Open",
    })[0]
    if (!found) throw new Error("no Open yet")
    return found
  })
  innerOpen.focus()
  await user.keyboard("{Enter}")
  await waitFor(() =>
    expect(section(view, "command").querySelector("code")?.textContent).toBe("npm test one"),
  )
  expect(document.activeElement).toBe(close)

  await user.keyboard("{Escape}")
  expect(screen.queryByRole("complementary", { name: "Full view" })).toBeNull()
  expect(document.activeElement).toBe(open)
  expect(requests.filter((one) => one.path.endsWith("/console/cancel"))).toEqual([])
})

it("keeps showing child tool progress after its inline subagent collapses", async () => {
  stubPanelWidth()
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  const { source } = renderTurn((turnId) => {
    subagent = aSubagentStep(turnId)
    return [subagent]
  })
  const child = aToolStep(subagent.turn_id, {
    lane: subagent.subagent_id,
    status: "running",
    ended_at: null,
  })
  subagentSteps[subagent.subagent_id] = [child]
  details[child.id] = {
    detail: { type: "command", command: "npm test", output: "before collapse", truncated: false },
  }
  act(() => resize(880))
  await user.click(row(subagent))
  await waitFor(() => row(child))
  await user.click(row(child))
  await user.click(within(detailOf(child) as HTMLElement).getByRole("button", { name: "Open" }))
  const view = screen.getByRole("complementary", { name: "Full view" })
  await within(view).findByText("before collapse")

  await user.click(row(subagent))
  expect(detailOf(subagent)).toBeNull()
  details[child.id] = {
    detail: { type: "command", command: "npm test", output: "after collapse", truncated: false },
  }
  act(() =>
    source.emit("update", anUpdate(2, [{ op: "item_upsert", step: { ...child, detail_rev: 2 } }])),
  )
  await within(view).findByText("after collapse")
})

it("returns focus to the step row when Escape closes a drawer whose opener disappeared", async () => {
  const user = userEvent.setup()
  let tool = {} as ToolStep
  renderTurn(
    (turnId) => {
      tool = aToolStep(turnId)
      return [tool]
    },
    { status: "running" },
  )
  details[tool.id] = { detail: { type: "command", command: "npm test", truncated: false } }
  const stepRow = row(tool)
  await user.click(stepRow)
  const open = within(detailOf(tool) as HTMLElement).getByRole("button", { name: "Open" })
  await user.click(open)
  await screen.findByRole("dialog", { name: tool.title })

  // Collapse the inline detail while the modal is open to unmount its opener.
  fireEvent.click(stepRow)
  expect(open.isConnected).toBe(false)
  await user.keyboard("{Escape}")
  await waitFor(() => expect(document.activeElement).toBe(stepRow))
  expect(screen.queryByRole("dialog")).toBeNull()
  expect(requests.filter((one) => one.path.endsWith("/console/cancel"))).toEqual([])
})

it("opens a subagent's steps in the scroll of the transcript, with no box of its own", async () => {
  const user = userEvent.setup()
  let subagent = aSubagentStep("none")
  renderTurn((turnId) => {
    subagent = aSubagentStep(turnId)
    return [subagent]
  })
  subagentSteps[subagent.subagent_id] = [
    aToolStep(subagent.turn_id, { lane: subagent.subagent_id }),
  ]

  await user.click(row(subagent))

  const detail = detailOf(subagent) as HTMLElement
  expect(detail.className).not.toContain("max-h-")
  expect(detail.className).not.toContain("overflow")
})
