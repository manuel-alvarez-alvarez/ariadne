// @vitest-environment jsdom

/** The status row under the transcript: one line, whatever the tool's name holds. */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, expect, it } from "vitest"

import { StatusRow } from "./status"

afterEach(cleanup)

it("holds the turn activity to half of the row, cut with an ellipsis, and never cuts a fact", () => {
  render(
    <StatusRow
      status="running"
      turn={{ tool: "Bash\nnpm run build", seconds: 3 }}
      tokens={{ input: 1200, output: 30 }}
      facts={[{ key: "context", text: "context 25%" }]}
    />,
  )

  const turn = screen.getByText("running Bash").parentElement as HTMLElement
  expect(turn.className).toContain("max-w-[50%]")
  expect(screen.getByText("running Bash").className).toContain("truncate")
  // A fact has no truncate and no shrink: it shows whole or drops out of the first line.
  const fact = screen.getByText("context 25%")
  expect(fact.className).not.toContain("truncate")
  const line = screen.getByRole("region", { name: "Session status" })
    .firstElementChild as HTMLElement
  expect(line.className).toContain("flex-wrap")
  // The clip is one line high and the padding sits outside it, so no part of a second line shows.
  expect(line.className).toContain("h-4")
  expect(line.className).toContain("overflow-hidden")
})

it("shows only the first line of a multiline tool name, whole on hover", async () => {
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
  expect((row.firstElementChild as HTMLElement).className).toContain("whitespace-nowrap")
  expect(row.textContent).toMatch(/↑.*↓/)

  await userEvent.setup().hover(shown)
  const whole = await screen.findByText(/npm run build/)
  expect(whole.textContent).toBe("Bash\nnpm run build\n--verbose")
})
