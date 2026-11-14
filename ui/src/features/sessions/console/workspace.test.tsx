// @vitest-environment jsdom

import { act, fireEvent, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"
import type { AgentEventDto } from "@/api"
import { latestSource, stubEventSource } from "@/test/event-source"
import { daemonFetch, renderScreen } from "@/test/harness"
import { SessionConsole } from "./session-console"

let next = 0
function event(kind: string, payload: unknown): AgentEventDto {
  return {
    id: `workspace-${++next}`,
    kind,
    payload,
    summary: kind,
    created_at: "2026-11-14T10:00:00Z",
  }
}
function tool(id: string, output = "first output") {
  return event("tool_call_update", {
    acp: {
      toolCallId: id,
      title: id,
      kind: "execute",
      status: "in_progress",
      content: [{ type: "content", content: { text: output } }],
    },
    tool_input: { command: `echo ${id}` },
  })
}
let resize: (width: number) => void
beforeEach(() => {
  stubEventSource()
  daemonFetch.mockImplementation(async () => Response.json([]))
  resize = () => {}
  vi.stubGlobal(
    "ResizeObserver",
    class {
      callback: ResizeObserverCallback
      constructor(callback: ResizeObserverCallback) {
        this.callback = callback
      }
      observe(target: Element) {
        if (target.hasAttribute("data-console-workspace"))
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
})
function renderConsole(events: AgentEventDto[] = [tool("Build")]) {
  renderScreen(<SessionConsole sessionId={`workspace-${++next}`} status="running" />)
  const source = latestSource()
  act(() => {
    source.succeed()
    source.emit("snapshot", events)
  })
  act(() => resize(1000))
  return source
}
function log() {
  return screen.getByRole("log", { name: "Console transcript" })
}

it("keeps compact tool rows and streams only the selected tool into adjacent details", () => {
  const source = renderConsole()
  expect(screen.queryByText("first output")).toBeNull()
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  const details = screen.getByRole("complementary", { name: "Tool details" })
  within(details).getByText("first output")
  expect(within(log()).queryByText("first output")).toBeNull()
  act(() => source.emit("event", tool("Test", "newer output")))
  expect(within(details).queryByText("newer output")).toBeNull()
  act(() => source.emit("event", tool("Build", "updated output")))
  within(details).getByText(/updated output/)
  expect(within(log()).getAllByRole("button")).toHaveLength(2)
  expect(within(log()).queryByText("updated output")).toBeNull()
})

it("opens details with the keyboard and returns focus when details close", async () => {
  const user = userEvent.setup()
  renderConsole()
  const row = within(log()).getByRole("button", { name: /Build/ })
  row.focus()
  await user.keyboard("{Enter}")
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close tool details" }))
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("complementary", { name: "Tool details" })).toBeNull()
  expect(document.activeElement).toBe(row)
})

it("uses a drawer at narrow panel widths and reveals a pending permission from it", () => {
  renderConsole([
    tool("Build"),
    event("permission_request", {
      tool_name: "Bash",
      tool_input: { command: "deploy" },
      options: [{ option_id: "allow", name: "Allow", kind: "allow_once" }],
    }),
  ])
  act(() => resize(500))
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  const drawer = screen.getByRole("dialog", { name: "Tool details" })
  fireEvent.click(within(drawer).getByRole("button", { name: /Pending request/ }))
  expect(screen.queryByRole("dialog", { name: "Tool details" })).toBeNull()
  expect(document.activeElement).toBe(
    screen.getByRole("listbox", { name: "Answer the permission" }),
  )
})

it("leaves Ctrl-O unhandled in embedded and expanded consoles", () => {
  const events = [event("agent_thought", { text: "first\nsecond" })]
  renderConsole(events)
  const press = (target: HTMLElement) => {
    expect(fireEvent.keyDown(target, { key: "o", ctrlKey: true })).toBe(true)
    expect(within(log()).queryByText(/first\nsecond/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Unfold" })).toBeNull()
  }
  press(screen.getByRole("textbox", { name: "Message the agent" }))
  press(screen.getByRole("button", { name: "Expand the console" }))
  fireEvent.click(screen.getByRole("button", { name: "Expand the console" }))
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", events)
  })
  press(screen.getByRole("textbox", { name: "Message the agent" }))
})

it("keeps selected details when resizing between a drawer and adjacent details", async () => {
  const user = userEvent.setup()
  renderConsole()
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  act(() => resize(500))
  within(screen.getByRole("dialog", { name: "Tool details" })).getByText("first output")
  act(() => resize(1000))
  within(screen.getByRole("complementary", { name: "Tool details" })).getByText("first output")
  act(() => resize(500))
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("dialog", { name: "Tool details" })).toBeNull()
  expect(document.activeElement).toBe(within(log()).getByRole("button", { name: /Build/ }))
})

it("uses the same selectable details in an expanded console", () => {
  const events = [tool("Build")]
  renderConsole(events)
  fireEvent.click(screen.getByRole("button", { name: "Expand the console" }))
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", events)
    resize(1200)
  })
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  const details = screen.getByRole("complementary", { name: "Tool details" })
  within(details).getByText("first output")
  expect(within(log()).queryByText("first output")).toBeNull()
})

it("reveals a subagent question and sends its answer through the request route", async () => {
  const child = { id: "child", title: "Explorer" }
  renderConsole([
    tool("Build"),
    event("subagent_update", { subagent: child, state: "running" }),
    event("agent_request", {
      request_id: "question-1",
      hint: "Choose a base",
      question: { text: "Which base?", options: [{ id: "main", label: "Main branch" }] },
      subagent: child,
    }),
  ])
  fireEvent.click(screen.getByRole("button", { name: /Pending request/ }))
  within(log()).getByText("Which base?")
  const answer = screen.getByRole("button", { name: "Main branch" })
  expect(document.activeElement).toBe(answer)
  const posted: string[] = []
  daemonFetch.mockImplementation(async (input: Request) => {
    if (input.method === "GET") return Response.json([])
    posted.push(`${input.method} ${new URL(input.url).pathname} ${await input.text()}`)
    return new Response(null, { status: 204 })
  })
  await userEvent.click(answer)
  expect(posted[0]).toContain('/agent-requests/question-1/answer {"option_id":"main"}')
})

it("returns from a child view to answer its pending permission in details", async () => {
  const child = { id: "child", title: "Explorer" }
  renderConsole([
    event("subagent_update", { subagent: child, state: "running" }),
    event("permission_request", {
      tool_name: "Bash",
      subagent: child,
      options: [{ optionId: "allow", name: "Allow" }],
    }),
  ])
  fireEvent.click(screen.getByRole("tab", { name: /Explorer/ }))
  fireEvent.click(screen.getByRole("button", { name: /Pending request/ }))
  const picker = screen.getByRole("listbox", { name: "Answer the permission" })
  expect(document.activeElement).toBe(picker)
  within(log()).getByText(/asked by subagent Explorer/)
  const posted: string[] = []
  daemonFetch.mockImplementation(async (input: Request) => {
    if (input.method === "GET") return Response.json([])
    posted.push(await input.text())
    return new Response(null, { status: 204 })
  })
  await userEvent.click(within(picker).getByRole("option", { name: /Allow/ }))
  expect(posted).toEqual(['{"text":"allow"}'])
})

it("shows existing input output and code changes in details without expanding the activity row", async () => {
  renderConsole([
    event("tool_call_update", {
      acp: {
        toolCallId: "edit",
        title: "Edit",
        kind: "edit",
        status: "completed",
        content: [
          { type: "content", content: { text: "Changed one line" } },
          { type: "diff", path: "a.ts", patch: { text: "@@ -1 +1 @@\n-old\n+new" } },
        ],
      },
      tool_input: { file_path: "a.ts", old_string: "old" },
    }),
  ])
  fireEvent.click(within(log()).getByRole("button", { name: /Edit/ }))
  const details = screen.getByRole("complementary", { name: "Tool details" })
  expect(
    Array.from(details.querySelectorAll("[data-code-caption]")).map((node) => node.textContent),
  ).toEqual(["input · JSON", "output", "diff"])
  within(details).getByText("+new")
  expect(log().querySelector("[data-code-block]")).toBeNull()
  await userEvent.click(within(details).getByRole("button", { name: "Copy diff" }))
  expect(screen.getByRole("complementary", { name: "Tool details" })).toBe(details)
})

it("keeps parent and child calls with the same tool id separate", () => {
  const child = { id: "child", title: "Explorer" }
  const childTool = tool("Build", "child output")
  childTool.payload = { ...(childTool.payload as object), subagent: child }
  renderConsole([
    tool("Build"),
    event("subagent_update", { subagent: child, state: "running" }),
    childTool,
  ])
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  within(screen.getByRole("complementary", { name: "Tool details" })).getByText("first output")
  fireEvent.click(screen.getByRole("tab", { name: /Explorer/ }))
  expect(screen.queryByRole("complementary", { name: "Tool details" })).toBeNull()
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  within(screen.getByRole("complementary", { name: "Tool details" })).getByText("child output")
})

it("closes a nested details drawer without closing the expanded console or cancelling", async () => {
  const user = userEvent.setup()
  const events = [tool("Build")]
  renderConsole(events)
  fireEvent.click(screen.getByRole("button", { name: "Expand the console" }))
  act(() => {
    latestSource().succeed()
    latestSource().emit("snapshot", events)
    resize(500)
  })
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  screen.getByRole("dialog", { name: "Tool details" })
  daemonFetch.mockClear()
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("dialog", { name: "Tool details" })).toBeNull()
  screen.getByRole("dialog", { name: "Session console" })
  expect(document.activeElement).toBe(within(log()).getByRole("button", { name: /Build/ }))
  expect(
    daemonFetch.mock.calls.some(
      ([input]) => input instanceof Request && input.url.endsWith("/console/cancel"),
    ),
  ).toBe(false)
})

it("keeps user developer and assistant messages always visible", () => {
  renderConsole([
    event("user_prompt_submit", { text: "User message in full" }),
    event("user_prompt_submit", { text: "Developer message in full", source: "daemon" }),
    event("agent_message", { text: "Assistant message in full" }),
  ])
  for (const label of ["You", "Developer", "Assistant"]) within(log()).getByText(label)
  for (const text of [
    "User message in full",
    "Developer message in full",
    "Assistant message in full",
  ])
    within(log()).getByText(text)
})

it("keeps the question and chosen option in an answered question summary", () => {
  renderConsole([
    event("agent_request", {
      request_id: "q",
      hint: "Choose",
      question: {
        text: "Which branch should receive this change?",
        options: [{ id: "main", label: "Main" }],
      },
    }),
    event("agent_request.resolved", {
      request_id: "q",
      resolution: "answered",
      answer: { option_id: "main", text: "Main" },
    }),
  ])
  const summary = within(log()).getByRole("button", {
    name: /Which branch should receive this change/,
  })
  expect(summary.textContent).toContain("Answered · Main")
})

it("opens permission response controls in details and keeps the conversation compact", async () => {
  const source = renderConsole([
    event("permission_request", {
      tool_name: "Bash",
      tool_input: { command: "deploy production" },
      options: [{ optionId: "allow", name: "Allow", kind: "allow_once" }],
    }),
  ])
  expect(within(log()).queryByRole("listbox")).toBeNull()
  fireEvent.click(within(log()).getByRole("button", { name: /Permission/ }))
  const details = screen.getByRole("complementary", { name: "Permission details" })
  const picker = within(details).getByRole("listbox", { name: "Answer the permission" })
  expect(document.activeElement).toBe(picker)
  within(details).getByText("deploy production")
  const posted: string[] = []
  daemonFetch.mockImplementation(async (input: Request) => {
    if (input.method === "GET") return Response.json([])
    posted.push(await input.text())
    return new Response(null, { status: 204 })
  })
  await userEvent.click(within(picker).getByRole("option", { name: /Allow/ }))
  expect(posted).toEqual(['{"text":"allow"}'])
  act(() =>
    source.emit(
      "event",
      event("permission.replied", { option_id: "allow", decided_by: "console" }),
    ),
  )
  expect(within(details).queryByRole("listbox")).toBeNull()
})

it("keeps tool selection when a permission arrives and opens its drawer only on request", async () => {
  const user = userEvent.setup()
  const source = renderConsole()
  act(() => resize(500))
  fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
  act(() =>
    source.emit(
      "event",
      event("permission_request", {
        tool_name: "Deploy",
        options: [{ optionId: "allow", name: "Allow" }],
      }),
    ),
  )
  const tools = screen.getByRole("dialog", { name: "Tool details" })
  fireEvent.click(within(tools).getByRole("button", { name: /Pending request/ }))
  const permission = screen.getByRole("dialog", { name: "Permission details" })
  expect(document.activeElement).toBe(within(permission).getByRole("listbox"))
  await user.keyboard("{Escape}")
  expect(screen.queryByRole("dialog", { name: "Permission details" })).toBeNull()
  screen.getByRole("textbox", { name: "Message the agent" })
  expect(document.activeElement).toBe(
    within(log()).getByRole("button", { name: /Permission · Deploy/ }),
  )
})

it("omits the global fold header button in embedded and expanded consoles", () => {
  renderConsole()
  expect(screen.queryByRole("button", { name: /^(Fold|Unfold)$/ })).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: "Expand the console" }))
  expect(screen.queryByRole("button", { name: /^(Fold|Unfold)$/ })).toBeNull()
})

it("omits the conversation strip while retaining the named transcript in both views", () => {
  renderConsole()
  screen.getByRole("log", { name: "Console transcript" })
  expect(screen.queryByText("Conversation", { exact: true })).toBeNull()
  fireEvent.click(screen.getByRole("button", { name: "Expand the console" }))
  screen.getByRole("log", { name: "Console transcript" })
  expect(screen.queryByText("Conversation", { exact: true })).toBeNull()
})

it.each([false, true])(
  "keeps hidden permission keys in the composer with another tool selected: %s",
  async (otherTool) => {
    const user = userEvent.setup()
    renderConsole([
      tool("Build"),
      event("permission_request", {
        tool_name: "Deploy",
        options: [
          { optionId: "allow", name: "Allow" },
          { optionId: "deny", name: "Deny" },
        ],
      }),
    ])
    if (otherTool) fireEvent.click(within(log()).getByRole("button", { name: /Build/ }))
    const posts: string[] = []
    daemonFetch.mockImplementation(async (request: Request) => {
      if (request.method !== "GET") posts.push(await request.text())
      return Response.json([])
    })
    const field = screen.getByRole("textbox", { name: "Message the agent" })
    await user.click(field)
    await user.keyboard("{Enter}")
    expect(posts).toEqual([])
    await user.keyboard("12")
    expect((field as HTMLTextAreaElement).value).toBe("12")
    expect(posts).toEqual([])
  },
)

it("hands empty composer keys to the displayed pending permission", async () => {
  const user = userEvent.setup()
  renderConsole([
    event("permission_request", {
      tool_name: "Deploy",
      options: [
        { optionId: "allow", name: "Allow" },
        { optionId: "deny", name: "Deny" },
      ],
    }),
  ])
  fireEvent.click(within(log()).getByRole("button", { name: /Permission/ }))
  const posts: string[] = []
  daemonFetch.mockImplementation(async (request: Request) => {
    if (request.method !== "GET") posts.push(await request.text())
    return Response.json([])
  })
  await user.click(screen.getByRole("textbox", { name: "Message the agent" }))
  await user.keyboard("2{Enter}")
  expect(posts).toEqual(['{"text":"deny"}'])
})

it("types digits normally with permission details closed", async () => {
  const user = userEvent.setup()
  renderConsole([
    event("permission_request", {
      tool_name: "Deploy",
      options: [{ optionId: "allow", name: "Allow" }],
    }),
  ])
  const field = screen.getByRole("textbox", { name: "Message the agent" })
  await user.type(field, "123456789")
  expect((field as HTMLTextAreaElement).value).toBe("123456789")
})

it.each([1000, 500])("stops permission shortcuts after closing details at %spx", async (width) => {
  const user = userEvent.setup()
  renderConsole([
    event("permission_request", {
      tool_name: "Deploy",
      options: [
        { optionId: "allow", name: "Allow" },
        { optionId: "deny", name: "Deny" },
      ],
    }),
  ])
  act(() => resize(width))
  fireEvent.click(within(log()).getByRole("button", { name: /Permission/ }))
  fireEvent.click(screen.getByRole("button", { name: "Close permission details" }))
  const posts: string[] = []
  daemonFetch.mockImplementation(async (request: Request) => {
    if (request.method !== "GET") posts.push(await request.text())
    return Response.json([])
  })
  const field = screen.getByRole("textbox", { name: "Message the agent" })
  await user.click(field)
  // Ordinary empty Enter is handled by the composer, but must not answer a permission.
  await user.keyboard("{Enter}")
  expect(posts).toEqual([])
  for (const key of ["ArrowUp", "ArrowDown", "1", "2"])
    expect(fireEvent.keyDown(field, { key })).toBe(true)
  await user.keyboard("12")
  expect((field as HTMLTextAreaElement).value).toBe("12")
  expect(posts).toEqual([])
})

it.each([1000, 500])(
  "answers through the visible permission picker's own keys at %spx",
  async (width) => {
    const user = userEvent.setup()
    renderConsole([
      event("permission_request", {
        tool_name: "Deploy",
        options: [
          { optionId: "allow", name: "Allow" },
          { optionId: "deny", name: "Deny" },
        ],
      }),
    ])
    act(() => resize(width))
    await user.click(within(log()).getByRole("button", { name: /Permission/ }))
    const picker = screen.getByRole("listbox", { name: "Answer the permission" })
    await waitFor(() => expect(document.activeElement).toBe(picker))
    if (width === 500) {
      // The modal drawer keeps the background composer outside the accessible dialog.
      expect(screen.queryByRole("textbox", { name: "Message the agent" })).toBeNull()
    }
    const posts: string[] = []
    daemonFetch.mockImplementation(async (request: Request) => {
      if (request.method !== "GET") posts.push(await request.text())
      return Response.json([])
    })
    await user.keyboard("2{ArrowUp}{ArrowDown}{Enter}")
    expect(posts).toEqual(['{"text":"deny"}'])
  },
)
