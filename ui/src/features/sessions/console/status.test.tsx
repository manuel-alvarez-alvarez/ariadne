// @vitest-environment jsdom

/** The status row under the transcript: one line, whatever the tool's name holds. */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import { StatusRow } from "./status"

afterEach(cleanup)

it("shows only the first line of a multiline tool name, whole on hover, with the tokens at the right end", async () => {
  render(
    <StatusRow
      status="running"
      turn={{ tool: "Bash\nnpm run build\n--verbose", seconds: 3 }}
      tokens={{ input: 1200, output: 30 }}
    />,
  )

  const shown = screen.getByText("running Bash")
  expect(shown.textContent).not.toContain("npm run build")
  expect(shown.className).toContain("truncate")
  const row = screen.getByRole("region", { name: "Session status" })
  expect(row.className).toContain("whitespace-nowrap")
  expect(row.lastElementChild?.textContent).toMatch(/↑.*↓/)

  await userEvent.setup().hover(shown)
  const whole = await screen.findByText(/npm run build/)
  expect(whole.textContent).toBe("Bash\nnpm run build\n--verbose")
})
