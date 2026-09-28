// @vitest-environment jsdom

/**
 * The allow/deny range control on its own: the two handles and the zones they
 * cut the track into, the danger marker a later task will set, and the
 * commit rules — a release or a left/Entered field sends the one field that
 * changed, and a refusal snaps the row back to what it was.
 */

import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { Toaster } from "@/components/ui/sonner"
import type { useUpdateAiPermissions } from "./queries"
import { ThresholdRange } from "./threshold-range"

type Mutate = ReturnType<typeof useUpdateAiPermissions>["mutate"]

let mutateMock: ReturnType<typeof vi.fn>
let mutate: Mutate

beforeEach(() => {
  mutateMock = vi.fn()
  mutate = mutateMock as unknown as Mutate
})

it("shows the track's three zones and the handles named for what they hold", () => {
  const { container } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )

  const allow = screen.getByRole("slider", { name: "Allow threshold" })
  const deny = screen.getByRole("slider", { name: "Deny threshold" })
  expect(allow).toBeDefined()
  expect(deny).toBeDefined()

  const green = container.querySelector(".bg-status-done") as HTMLElement
  const amber = container.querySelector(".bg-status-warn") as HTMLElement
  const red = container.querySelector(".bg-status-danger") as HTMLElement
  expect(green.style.width).toBe("20%")
  expect(amber.style.left).toBe("20%")
  expect(amber.style.width).toBe("60%")
  expect(red.style.left).toBe("80%")

  expect(screen.getByText("Allow")).toBeDefined()
  expect(screen.getByText("Ask")).toBeDefined()
  expect(screen.getByText("Deny")).toBeDefined()
})

it("shows the two number inputs, named and valued for the row they hold", () => {
  render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  const deny = screen.getByRole("spinbutton", { name: "Deny threshold" }) as HTMLInputElement
  expect(allow.value).toBe("0.2")
  expect(deny.value).toBe("0.8")
})

it("draws no danger marker where none is set, and one labelled with its value to four decimals where it is", () => {
  const { rerender } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )
  expect(screen.queryByLabelText(/^Danger/)).toBeNull()

  rerender(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} danger={0.42} mutate={mutate} />,
  )
  expect(screen.getByLabelText("Danger 0.4200")).toBeDefined()
})

it("positions zone labels at the center of each zone and updates them when thresholds move", () => {
  const { container, rerender } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )

  let labels = Array.from(
    container.querySelectorAll("span.pointer-events-none.absolute.top-0"),
  ) as HTMLElement[]
  expect(labels.length).toBe(3)
  expect(labels[0]?.textContent).toBe("Allow")
  expect(labels[1]?.textContent).toBe("Ask")
  expect(labels[2]?.textContent).toBe("Deny")

  // Labels positioned absolutely with left: Allow at 10%, Ask at 50%, Deny at 90%
  expect(labels[0]?.className).toContain("absolute")
  expect(labels[0]?.style.left).toBe("10%")
  expect(labels[1]?.className).toContain("absolute")
  expect(labels[1]?.style.left).toBe("50%")
  expect(labels[2]?.className).toContain("absolute")
  expect(labels[2]?.style.left).toBe("90%")

  rerender(<ThresholdRange allowThreshold={0.3} denyThreshold={0.7} mutate={mutate} />)

  labels = Array.from(
    container.querySelectorAll("span.pointer-events-none.absolute.top-0"),
  ) as HTMLElement[]
  expect(labels[0]?.style.left).toBe("15%")
  expect(labels[1]?.style.left).toBe("50%")
  expect(labels[2]?.style.left).toBe("85%")
})

it("hides only the narrow zone label", () => {
  const { container } = render(
    <ThresholdRange allowThreshold={0.45} denyThreshold={0.55} mutate={mutate} />,
  )

  const labels = Array.from(
    container.querySelectorAll("span.pointer-events-none.absolute.top-0"),
  ) as HTMLElement[]
  expect(labels.length).toBe(3)
  // Ask zone is 10% wide (< 15%), so only Ask is hidden
  expect(labels[0]?.className).toContain("opacity-100")
  expect(labels[1]?.className).toContain("opacity-0")
  expect(labels[2]?.className).toContain("opacity-100")
})

it("shows ticks and numbers at 0, 0.5 and 1 under the track", () => {
  const { container } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )

  // Check numbers are present
  expect(screen.getByText("0")).toBeDefined()
  expect(screen.getByText("0.5")).toBeDefined()
  expect(screen.getByText("1")).toBeDefined()

  // Check tick marks are present (visual lines with bg-muted-foreground)
  const tickContainer = container.querySelector("div.pointer-events-none.absolute.bottom-0")
  expect(tickContainer).toBeDefined()
  const ticks = Array.from(
    tickContainer?.querySelectorAll("span.bg-muted-foreground") || [],
  ) as HTMLElement[]
  expect(ticks.length).toBe(3)
})

it("gives each thumb an aria-valuetext with the value to four decimals", () => {
  const { rerender } = render(
    <ThresholdRange allowThreshold={0.2456} denyThreshold={0.7654} mutate={mutate} />,
  )

  const allow = screen.getByRole("slider", { name: "Allow threshold" }) as HTMLElement
  const deny = screen.getByRole("slider", { name: "Deny threshold" }) as HTMLElement
  expect(allow.getAttribute("aria-valuetext")).toBe("Allow threshold 0.2456")
  expect(deny.getAttribute("aria-valuetext")).toBe("Deny threshold 0.7654")

  rerender(<ThresholdRange allowThreshold={0.1} denyThreshold={0.9} mutate={mutate} />)

  const allowUpdated = screen.getByRole("slider", { name: "Allow threshold" }) as HTMLElement
  const denyUpdated = screen.getByRole("slider", { name: "Deny threshold" }) as HTMLElement
  expect(allowUpdated.getAttribute("aria-valuetext")).toBe("Allow threshold 0.1000")
  expect(denyUpdated.getAttribute("aria-valuetext")).toBe("Deny threshold 0.9000")
})

describe("a handle released at a new value", () => {
  it("sends one PUT with only the allow field", async () => {
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const allow = screen.getByRole("slider", { name: "Allow threshold" })
    allow.focus()
    await userEvent.keyboard("{ArrowRight}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ allow_threshold: 0.21 }, expect.anything())
  })

  it("sends one PUT with only the deny field", async () => {
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const deny = screen.getByRole("slider", { name: "Deny threshold" })
    deny.focus()
    await userEvent.keyboard("{ArrowLeft}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ deny_threshold: 0.79 }, expect.anything())
  })

  it("does not let the allow handle reach or pass the deny handle", async () => {
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.21} mutate={mutate} />)

    const allow = screen.getByRole("slider", { name: "Allow threshold" })
    allow.focus()
    await userEvent.keyboard("{End}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    const [body] = mutateMock.mock.calls[0] as [{ allow_threshold: number }]
    expect(body.allow_threshold).toBeLessThan(0.21)
  })

  it("does not let the deny handle reach or pass the allow handle", async () => {
    render(<ThresholdRange allowThreshold={0.79} denyThreshold={0.85} mutate={mutate} />)

    const deny = screen.getByRole("slider", { name: "Deny threshold" })
    deny.focus()
    await userEvent.keyboard("{Home}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    const [body] = mutateMock.mock.calls[0] as [{ deny_threshold: number }]
    expect(body.deny_threshold).toBeGreaterThan(0.79)
  })
})

describe("an input left or Entered", () => {
  it("sends one PUT with only the allow field, four decimals kept", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const allow = screen.getByRole("spinbutton", { name: "Allow threshold" })
    await user.clear(allow)
    await user.type(allow, "0.1338")
    await user.tab()

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ allow_threshold: 0.1338 }, expect.anything())
  })

  it("sends one PUT with only the deny field, on Enter", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const deny = screen.getByRole("spinbutton", { name: "Deny threshold" })
    await user.clear(deny)
    await user.type(deny, "0.5345{Enter}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ deny_threshold: 0.5345 }, expect.anything())
  })

  it("rounds a fifth allow decimal to four before sending", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const allow = screen.getByRole("spinbutton", { name: "Allow threshold" })
    await user.clear(allow)
    await user.type(allow, "0.12345")
    await user.tab()

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ allow_threshold: 0.1235 }, expect.anything())
  })

  it("rounds a fifth deny decimal to four before sending", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const deny = screen.getByRole("spinbutton", { name: "Deny threshold" })
    await user.clear(deny)
    await user.type(deny, "0.98765{Enter}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ deny_threshold: 0.9877 }, expect.anything())
  })

  it("clamps a typed allow value of 5 to 1 before sending", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const allow = screen.getByRole("spinbutton", { name: "Allow threshold" })
    await user.clear(allow)
    await user.type(allow, "5")
    await user.tab()

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ allow_threshold: 1 }, expect.anything())
  })

  it("clamps a typed deny value of -1 to 0 before sending", async () => {
    const user = userEvent.setup()
    render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

    const deny = screen.getByRole("spinbutton", { name: "Deny threshold" })
    await user.clear(deny)
    await user.type(deny, "-1{Enter}")

    expect(mutateMock).toHaveBeenCalledTimes(1)
    expect(mutateMock).toHaveBeenCalledWith({ deny_threshold: 0 }, expect.anything())
  })
})

it("toasts the daemon's own message on a refusal, and puts the value back", async () => {
  mutateMock.mockImplementation((_body, options) => {
    options.onError(new Error("the allow threshold must stay under the deny threshold"))
  })
  const user = userEvent.setup()
  render(
    <>
      <Toaster />
      <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />
    </>,
  )

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  await user.clear(allow)
  await user.type(allow, "0.95")
  await user.tab()

  expect(
    await screen.findByText(/the allow threshold must stay under the deny threshold/),
  ).toBeDefined()
  await waitFor(() => expect(allow.value).toBe("0.2"))
})
