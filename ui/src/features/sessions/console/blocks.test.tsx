// @vitest-environment jsdom

/**
 * The console's blocks, drawn from the transcript the console stream folds:
 * a tool call as one frame, its sections, its summary line and every
 * block's time.
 */

import { fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, expect, it, vi } from "vitest"

import type { AgentEventDto } from "@/api"

import { BlockView } from "./blocks"
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

it("draws a tool call in one frame, with its input, output and diff bare under their captions", () => {
  const { container } = drawn(edit())

  const frames = Array.from(container.querySelectorAll("*")).filter(
    (node) => node.tagName !== "BUTTON" && framed(node),
  )
  expect(frames).toHaveLength(1)
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

it("names a call's status, kind and subject at the left of its summary, and its count, duration and time at the right", () => {
  const blocks = fold(
    call("run", { title: "Bash", kind: "execute", status: "pending" }, at(9, 5, 7), {
      command: "cargo test\n--all",
    }),
    call(
      "run",
      { status: "completed", content: [{ type: "content", content: { text: "a\nb\nc" } }] },
      at(9, 5, 7, 340),
    ),
  )
  drawn(blocks[0] as Block, true)

  const summary = screen.getByRole("button", { expanded: false })
  expect(summary.querySelector("[data-tool-kind]")?.getAttribute("data-tool-kind")).toBe("execute")
  expect(within(summary).getByText("cargo test").className).toContain("text-foreground")
  expect(summary.querySelector("[data-summary-end]")?.textContent).toBe("3 lines340ms09:05:07")
  expect(summary.textContent).toMatch(/^completedexecutecargo test/)
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
    const { container, unmount } = drawn(blocks[0] as Block)
    const end = container.querySelector("[data-summary-end]")?.textContent
    unmount()
    return end
  }

  expect(took(at(10, 0, 2, 100))).toBe("2.1s10:00:00")
  expect(took(at(10, 1, 4))).toBe("1m 04s10:00:00")
  const running = fold(call("c", { title: "Bash", kind: "execute", status: "in_progress" }))
  const { container } = drawn(running[0] as Block)
  expect(container.querySelector("[data-summary-end]")?.textContent).toBe("10:00:00")
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

it("renders an unfolded daemon prompt as Markdown and keeps its summary plain", () => {
  const [block] = fold(event("user_prompt_submit", { text: "## Resume\n- one", source: "daemon" }))
  const { container } = drawn(block as Block, false)

  expect(container.querySelector("h2")?.textContent).toBe("Resume")
  expect(screen.getByRole("button").querySelector("h2")).toBeNull()
})

it("draws <kbd> in a prompt as a kbd element, and leaves other raw HTML as text", () => {
  const [block] = fold(event("user_prompt_submit", { text: "Press <kbd>Ctrl</kbd> and <b>x</b>" }))
  const { container } = drawn(block as Block)

  expect(container.querySelector("kbd")?.textContent).toBe("Ctrl")
  expect(container.querySelector("b")).toBeNull()
})
