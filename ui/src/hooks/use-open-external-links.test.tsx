// @vitest-environment jsdom

import { fireEvent, render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it, vi } from "vitest"

import { useOpenExternalLinks } from "./use-open-external-links"

const { isTauri, openUrl } = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  openUrl: vi.fn(),
}))

vi.mock("@tauri-apps/api/core", () => ({ isTauri }))
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }))

function Screen() {
  const { confirmation } = useOpenExternalLinks()
  return (
    <>
      <a href="https://example.com/pr/1">Pull request</a>
      <a href="https://example.com/direct">https://example.com/direct</a>
      <a href="mailto:security@example.com">Report a problem</a>
      <a href="file:///tmp/secret">File</a>
      <a href="https://example.com/blocked-script">Script</a>
      <a href="ariadne://settings">Custom</a>
      <a href="#/goals">Goals</a>
      {confirmation}
    </>
  )
}

function mount() {
  isTauri.mockReturnValue(true)
  openUrl.mockClear()
  const view = render(<Screen />)
  view.getByText("Script").setAttribute("href", ["java", "script:alert('unsafe')"].join(""))
  return view
}

afterEach(() => {
  vi.restoreAllMocks()
})

it("shows the full target before opening a disguised external link", async () => {
  const user = userEvent.setup()
  const { getByText } = mount()

  await user.click(getByText("Pull request"))

  expect(openUrl).not.toHaveBeenCalled()
  const dialog = screen.getByRole("dialog", { name: "Open external link?" })
  expect(within(dialog).getByText("https://example.com/pr/1")).not.toBeNull()

  await user.click(within(dialog).getByRole("button", { name: "Open link" }))

  expect(openUrl).toHaveBeenCalledWith("https://example.com/pr/1")
})

it("opens an external link directly when its text is its target", async () => {
  const user = userEvent.setup()
  const { getByText } = mount()

  await user.click(getByText("https://example.com/direct"))

  expect(openUrl).toHaveBeenCalledWith("https://example.com/direct")
  expect(screen.queryByRole("dialog", { name: "Open external link?" })).toBeNull()
})

it("allows mailto links through the same confirmation", async () => {
  const user = userEvent.setup()
  const { getByText } = mount()

  await user.click(getByText("Report a problem"))

  const dialog = screen.getByRole("dialog", { name: "Open external link?" })
  expect(within(dialog).getByText("mailto:security@example.com")).not.toBeNull()
  await user.click(within(dialog).getByRole("button", { name: "Open link" }))

  expect(openUrl).toHaveBeenCalledWith("mailto:security@example.com")
})

it("blocks unsafe link schemes before the webview follows them", () => {
  const { getByText } = mount()

  for (const label of ["File", "Script", "Custom"]) {
    expect(fireEvent.click(getByText(label))).toBe(false)
  }

  expect(openUrl).not.toHaveBeenCalled()
})

it("ignores an internal link", async () => {
  const user = userEvent.setup()
  const { getByText } = mount()

  await user.click(getByText("Goals"))

  expect(openUrl).not.toHaveBeenCalled()
})

it("leaves a plain browser's link behavior alone", async () => {
  isTauri.mockReturnValue(false)
  const user = userEvent.setup()
  const { getByText } = render(<Screen />)

  await user.click(getByText("Pull request"))

  expect(openUrl).not.toHaveBeenCalled()
})
