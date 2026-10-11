// @vitest-environment jsdom

import { render } from "@testing-library/react"
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
  useOpenExternalLinks()
  return (
    <>
      <a href="https://example.com/pr/1">Pull request</a>
      <a href="#/goals">Goals</a>
    </>
  )
}

function mount() {
  isTauri.mockReturnValue(true)
  openUrl.mockClear()
  return render(<Screen />)
}

afterEach(() => {
  vi.restoreAllMocks()
})

it("opens an external link through the opener plugin", async () => {
  const user = userEvent.setup()
  const { getByText } = mount()

  await user.click(getByText("Pull request"))

  expect(openUrl).toHaveBeenCalledWith("https://example.com/pr/1")
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
