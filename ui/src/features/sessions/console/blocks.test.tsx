// @vitest-environment jsdom

/**
 * The console's blocks, drawn from the transcript the console stream folds:
 * a tool call with its border, sections, summary line and every
 * block's time.
 */

import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import type { AgentEventDto } from "@/api"

import { BlockView, ToolRow } from "./blocks"
import { applyEvent, type Block, emptyTranscript } from "./transcript"

const rendered = vi.hoisted(() => [] as string[])

// Count each render of a message's markdown, to see which blocks a chunk re-renders.
vi.mock("@/components/markdown", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/components/markdown")>()
  return {
    Markdown: (props: { children: string; className?: string }) => {
      rendered.push(props.children)
      return <actual.Markdown {...props} />
    },
  }
})

afterEach(() => {
  vi.unstubAllGlobals()
  rendered.length = 0
})

/** An instant of the local clock, so its `HH:MM:SS` is the same in every time zone. */
function at(hours: number, minutes: number, seconds: number, millis = 0): string {
  return new Date(2026, 10, 14, hours, minutes, seconds, millis).toISOString()
}

let next = 0
function event(kind: string, payload: unknown, created_at = at(10, 0, 0)): AgentEventDto {
  next += 1
  return {
    id: `01EVENT${String(next).padStart(19, "0")}`,
    kind,
    payload,
    summary: `${kind} summary`,
    created_at,
  }
}

function fold(...events: AgentEventDto[]): Block[] {
  return events.reduce(applyEvent, emptyTranscript()).blocks
}

function drawn(block: Block, folded = false, onToggle = () => {}) {
  return render(<BlockView block={block} picker={null} folded={folded} onToggle={onToggle} />)
}

function row(block: Block) {
  if (block.type !== "tool") throw new Error("Expected a tool")
  return <ToolRow block={block} selected={false} onSelect={() => {}} />
}

function call(id: string, acp: Record<string, unknown>, created_at?: string, input?: unknown) {
  return event(
    "tool_call_update",
    { acp: { toolCallId: id, ...acp }, ...(input === undefined ? {} : { tool_input: input }) },
    created_at,
  )
}

const edit = () =>
  fold(
    call(
      "edit",
      {
        title: "Edit",
        kind: "edit",
        status: "completed",
        content: [
          { type: "content", content: { text: '{"ok":true}' } },
          { type: "diff", path: "a.ts", patch: { text: "@@ -1 +1 @@\n-old\n+new" } },
        ],
      },
      undefined,
      { file_path: "a.ts", old_string: "old" },
    ),
  )[0] as Block

/** Whether a class list sets a border or a background of its own, not on hover or focus. */
const framed = (node: Element) =>
  /(^|\s)(border|border-[blrtxy]|bg-[\w/[\]-]+)(\s|$)/.test(node.getAttribute("class") ?? "")

it("draws tool details with captioned sections and no left border", () => {
  const { container } = drawn(edit())

  const frames = Array.from(container.querySelectorAll("*")).filter(
    (node) => node.tagName !== "BUTTON" && framed(node),
  )
  expect(frames).toHaveLength(0)
  const sections = Array.from(container.querySelectorAll("[data-code-block]"))
  expect(
    sections.map((section) => section.querySelector("[data-code-caption]")?.textContent),
  ).toEqual(["input · JSON", "output · JSON", "diff"])
  expect(sections[0]?.querySelector("code")?.textContent).toBe(
    '{\n  "file_path": "a.ts",\n  "old_string": "old"\n}',
  )
  expect(sections[1]?.querySelector("code")?.textContent).toBe('{\n  "ok": true\n}')
})

it("copies one section's text from its icon-only control and never toggles the fold", async () => {
  const written: string[] = []
  vi.stubGlobal("navigator", {
    ...navigator,
    clipboard: { writeText: async (text: string) => void written.push(text) },
  })
  const onToggle = vi.fn()
  drawn(edit(), false, onToggle)

  const copy = screen.getByRole("button", { name: "Copy input" })
  expect(copy.textContent).toBe("")
  fireEvent.click(copy)
  fireEvent.click(screen.getByRole("button", { name: "Copy diff" }))

  await vi.waitFor(() =>
    expect(written).toEqual([
      '{\n  "file_path": "a.ts",\n  "old_string": "old"\n}',
      "--- a.ts\n+++ a.ts\n@@ -1 +1 @@\n-old\n+new",
    ]),
  )
  expect(onToggle).not.toHaveBeenCalled()
})

it("places the tool kind and status before its name and short summary", () => {
  const [block] = fold(
    call("run", { title: "Bash", kind: "execute", status: "pending" }, at(9, 5, 7), {
      command: "cargo test\n--all",
    }),
  )
  const { container } = render(row(block as Block))
  const summary = screen.getByRole("button", { pressed: false })
  expect(summary.textContent).toContain("executependingBashcargo test")
  expect(container.querySelector("[data-tool-kind=execute]")).not.toBeNull()
})

it("gives every ACP kind its own icon and any other kind one more", () => {
  const kinds = [
    "read",
    "edit",
    "delete",
    "move",
    "search",
    "execute",
    "think",
    "fetch",
    "switch_mode",
  ]
  const icons = kinds.map((kind) => {
    const { container, unmount } = drawn(
      fold(call(kind, { title: kind, kind, status: "pending" }))[0] as Block,
    )
    const icon = container.querySelector("[data-tool-kind] svg")?.getAttribute("class")
    unmount()
    return icon
  })

  expect(new Set(icons).size).toBe(kinds.length)
})

it("says how long a call took in the unit that fits it, once it has ended", () => {
  const took = (ended: string) => {
    const blocks = fold(
      call("c", { title: "Bash", kind: "execute", status: "in_progress" }, at(10, 0, 0)),
      call("c", { status: "completed" }, ended),
    )
    const { container, unmount } = render(row(blocks[0] as Block))
    const end = container.querySelector("[data-summary-end]")?.textContent
    unmount()
    return end
  }

  expect(took(at(10, 0, 2, 100))).toBe("2.1s")
  expect(took(at(10, 1, 4))).toBe("1m 04s")
  const running = fold(call("c", { title: "Bash", kind: "execute", status: "in_progress" }))
  const { container } = render(row(running[0] as Block))
  expect(container.querySelector("[data-summary-end]")?.textContent).toBeTruthy()
})

it("shows a read's output with no input, in its file's language, numbered from its first line outside the text", () => {
  const blocks = fold(
    call(
      "read",
      {
        title: "Read",
        kind: "read",
        status: "completed",
        locations: [{ path: "src/main.rs" }],
        content: [
          { type: "content", content: { text: "```\n    41\tfn main() {\n    42\t}\n```" } },
        ],
      },
      undefined,
      { file_path: "src/main.rs" },
    ),
  )
  const { container } = drawn(blocks[0] as Block)

  const sections = Array.from(container.querySelectorAll("[data-code-block]"))
  expect(sections).toHaveLength(1)
  expect(sections[0]?.querySelector("[data-code-caption]")?.textContent).toBe("output · Rust")
  const lines = Array.from(sections[0]?.querySelectorAll("[data-line]") ?? [])
  expect(lines.map((line) => line.getAttribute("data-line"))).toEqual(["41", "42"])
  expect(sections[0]?.querySelector("code")?.textContent).toBe("fn main() {\n}")
})

it("shows a permission's command as shell", () => {
  const blocks = fold(
    event("permission_request", {
      tool_name: "Bash",
      tool_input: { command: "rm -rf target" },
      options: [{ optionId: "allow", name: "Allow" }],
    }),
  )
  const { container } = drawn(blocks[0] as Block)

  expect(container.querySelector("[data-code-caption]")?.textContent).toBe("command · Shell")
  expect(container.querySelector("code")?.textContent).toBe("rm -rf target")
})

it("shows every block's time: the clock on a prompt and a summary line, and the full date on hover", () => {
  const [prompt, thought] = fold(
    event("user_prompt_submit", { text: "Fix it" }, at(8, 30, 15)),
    event("agent_thought", { text: "pondering" }, at(8, 30, 16)),
  )
  const { container } = render(
    <>
      <BlockView block={prompt as Block} picker={null} folded onToggle={() => {}} />
      <BlockView block={thought as Block} picker={null} folded onToggle={() => {}} />
    </>,
  )

  const [shownPrompt, shownThought] = Array.from(container.querySelectorAll("[data-block]"))
  expect(shownPrompt?.querySelector("[data-block-time]")?.textContent).toBe("08:30:15")
  expect(shownThought?.querySelector("[data-summary-end]")?.textContent).toBe("08:30:16")
  expect(shownPrompt?.getAttribute("title")).toMatch(/14.*2026.*8:30:15/)
})

it("re-renders only the block a streamed chunk changes", () => {
  const onToggle = () => {}
  const Blocks = ({ blocks }: { blocks: Block[] }) => (
    <>
      {blocks.map((block) => (
        <BlockView key={block.id} block={block} picker={null} folded onToggle={onToggle} />
      ))}
    </>
  )
  let transcript = [
    event("agent_message", { text: "first" }),
    event("agent_message_chunk", { text: "sec" }),
  ].reduce(applyEvent, emptyTranscript())
  const { rerender } = render(<Blocks blocks={transcript.blocks} />)
  rendered.length = 0

  transcript = applyEvent(transcript, event("agent_message_chunk", { text: "ond" }))
  rerender(<Blocks blocks={transcript.blocks} />)

  expect(rendered).toEqual(["second"])
})

it("renders a prompt as Markdown: headings, task lists, a fenced block, and a single newline still breaks", () => {
  const text = "# Plan\n- [x] done\n- [ ] todo\n\nline one\nline two\n\n```ts\nconst a = 1\n```"
  const [block] = fold(event("user_prompt_submit", { text }))
  const { container } = drawn(block as Block)

  expect(container.querySelector("h1")?.textContent).toBe("Plan")
  expect(container.querySelectorAll("input[type=checkbox]")).toHaveLength(2)
  expect(container.querySelector("[data-code-block] code")?.textContent).toBe("const a = 1")
  const paragraph = container.querySelector("p")
  expect(paragraph?.textContent).toBe("line one\nline two")
  expect(paragraph?.parentElement?.className).toContain("[&_p]:whitespace-pre-wrap")
})

it("renders developer prompts as full Markdown without a fold", () => {
  const [block] = fold(event("user_prompt_submit", { text: "## Resume\n- one", source: "daemon" }))
  const { container } = drawn(block as Block, false)

  expect(container.querySelector("h2")?.textContent).toBe("Resume")
  expect(screen.queryByRole("button")).toBeNull()
  screen.getByText("Developer")
})

it("draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text", () => {
  const [block] = fold(event("user_prompt_submit", { text: "Press <kbd>Ctrl</kbd> and <b>x</b>" }))
  const { container } = drawn(block as Block)

  expect(container.querySelector("kbd")?.textContent).toBe("Ctrl")
  expect(container.querySelector("b")).toBeNull()
})

const LONG = `${"packages/very/deep/".repeat(12)}file.ts`

it("keeps a tool row compact with its full subject on hover", () => {
  const [block] = fold(
    call(
      "long",
      { title: "Read", kind: "read", status: "completed", locations: [{ path: LONG }] },
      at(10, 0, 0),
    ),
  )
  render(row(block as Block))
  const button = screen.getByRole("button")
  expect(button.getAttribute("title")).toContain(LONG)
  expect(button.querySelector("[data-summary-end]")?.textContent).toBe("0ms")
  expect(button.querySelectorAll(".truncate")).toHaveLength(2)
})

it("shows a call's full subject in its body where its input does not show it", () => {
  const [read] = fold(
    call("r", { title: "Read", kind: "read", status: "completed", content: [] }, undefined, {
      file_path: LONG,
    }),
  )
  vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockReturnValue(500)
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(100)
  const { container, unmount } = drawn(read as Block, false)
  expect(container.querySelector("[data-tool-subject]")?.textContent).toBe(LONG)
  unmount()

  const [shell] = fold(
    call("s", { title: "Bash", kind: "execute", status: "completed", content: [] }, undefined, {
      command: "echo hello",
    }),
  )
  const shown = drawn(shell as Block, false)
  expect(shown.container.querySelector("[data-tool-subject]")).toBeNull()
  expect(shown.container.querySelector("code")?.textContent).toBe("echo hello")
})

it("allows selecting a call even when it has no content", () => {
  const [block] = fold(call("empty", { title: "Read", kind: "read", status: "completed" }))
  if (block?.type !== "tool") throw new Error("Expected a tool")
  const select = vi.fn()
  render(<ToolRow block={block} selected={false} onSelect={select} />)
  const button = screen.getByRole("button")
  fireEvent.click(button)
  expect(select).toHaveBeenCalledWith(block.id, button)
})

const permissionEvents = (extra: Record<string, unknown> = {}) => [
  event(
    "permission_request",
    {
      tool_name: "Bash",
      tool_input: { command: "rm -rf target" },
      options: [{ optionId: "allow", name: "Allow" }],
      label: "ask",
      danger: 0.21,
      probabilities: { "0": 0.62, "1": 0.33, "2": 0.05 },
      allow_threshold: 0.05,
      deny_threshold: 0.65,
      ...extra,
    },
    at(9, 0, 0),
  ),
]

it("keeps a permission header on one line with its time, and shows the AI note", () => {
  const [block] = fold(...permissionEvents())
  const { container } = render(
    <BlockView block={block as Block} picker={<i data-picker />} folded onToggle={() => {}} />,
  )

  const end = container.querySelector("[data-summary-end]")
  expect(end?.textContent).toBe("09:00:00")
  expect(end?.previousElementSibling?.className).toContain("truncate")
  expect(end?.previousElementSibling?.getAttribute("title")).toMatch(
    /^Permission · Bash\n.*2026.*9:00:00/,
  )
  expect(container.textContent).toContain(
    "AI said ask · allow 62%, deny 5% · danger 21% (allow up to 5%, deny from 65%)",
  )
})

it("keeps a pending permission open whatever the fold, and folds an answered one to one line", () => {
  const pending = fold(...permissionEvents())[0] as Block
  const open = render(
    <BlockView block={pending} picker={<i data-picker />} folded onToggle={() => {}} />,
  )
  expect(open.container.querySelector("[data-picker]")).not.toBeNull()
  expect(open.container.querySelector("code")?.textContent).toBe("rm -rf target")
  open.unmount()

  const answered = fold(
    ...permissionEvents(),
    event("permission.replied", { option_id: "allow", decided_by: "console" }, at(9, 0, 5)),
  )[0] as Block
  const onToggle = vi.fn()
  const closed = render(<BlockView block={answered} picker={null} folded onToggle={onToggle} />)
  expect(closed.container.querySelector("code")).toBeNull()
  expect(closed.container.querySelector("[data-summary-end]")?.textContent).toBe(
    "Allow · decided by console09:00:00",
  )
  fireEvent.click(screen.getByRole("button"))
  expect(onToggle).toHaveBeenCalled()
  closed.unmount()

  const unfolded = drawn(answered, false)
  expect(unfolded.container.querySelector("code")?.textContent).toBe("rm -rf target")
})

it("updates a running tool duration and freezes it on completion", () => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date(at(10, 0, 1)))
  const running = fold(call("c", { title: "Run", status: "in_progress" }, at(10, 0, 0)))[0] as Block
  const { rerender, unmount } = render(row(running))
  expect(screen.getByText("1.0s")).toBeTruthy()
  act(() => vi.advanceTimersByTime(1000))
  expect(screen.getByText("2.0s")).toBeTruthy()
  const finished = fold(
    call("c", { title: "Run", status: "in_progress" }, at(10, 0, 0)),
    call("c", { status: "completed" }, at(10, 0, 2)),
  )[0] as Block
  rerender(row(finished))
  vi.advanceTimersByTime(5000)
  expect(screen.getByText("2.0s")).toBeTruthy()
  unmount()
  vi.useRealTimers()
})

it("shows a repeated tool command only in input", () => {
  const [block] = fold(
    call("run", { title: "Run echo hello", kind: "execute", status: "completed" }, undefined, {
      command: "echo hello",
    }),
  )
  const { container } = drawn(block as Block)
  expect(container.querySelector("h4")?.textContent).toBe("Tool call")
  expect(container.querySelector("code")?.textContent).toContain("echo hello")
})

it("shows a repeated permission command only in its command section", () => {
  const [block] = fold(
    event("permission_request", { tool_name: "echo hello", tool_input: { command: "echo hello" } }),
  )
  const { container } = drawn(block as Block)
  expect(container.querySelector("[data-summary-end]")?.previousElementSibling?.textContent).toBe(
    "Permission",
  )
  expect(container.querySelector("code")?.textContent).toContain("echo hello")
})

it.each(["sh", "pwd"])("keeps descriptive tool headings for %s input", (command) => {
  const [tool] = fold(
    call("short", { title: "Bash", kind: "execute", status: "completed" }, undefined, { command }),
  )
  const shown = drawn(tool as Block)
  expect(shown.container.querySelector("h4")?.textContent).toBe("Bash")
  shown.unmount()
})

it.each(["sh", "pwd"])("keeps descriptive permission headings for %s input", (command) => {
  const [permission] = fold(
    event("permission_request", { tool_name: "Bash", tool_input: { command } }),
  )
  const { container } = drawn(permission as Block)
  expect(container.querySelector("[data-summary-end]")?.previousElementSibling?.textContent).toBe(
    "Permission · Bash",
  )
})
