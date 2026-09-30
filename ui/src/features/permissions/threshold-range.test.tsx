// @vitest-environment jsdom

/**
 * The allow/deny range control on its own: the two handles and the zones they
 * cut the track into, the danger marker a later task will set, and the
 * commit rules — a release or a left/Entered field sends the one field that
 * changed, and a refusal snaps the row back to what it was.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react"
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

it("puts the zone labels on the track itself, centered on its own vertical middle", () => {
  const { container } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )

  const track = container.querySelector('[data-slot="slider-track"]') as HTMLElement
  for (const text of ["Allow", "Ask", "Deny"]) {
    const label = screen.getByText(text)
    expect(track.contains(label)).toBe(true)
    // "top-1/2 -translate-y-1/2" centers the label on the track's own
    // height, so it sits on the coloured bar rather than above or below it.
    expect(label.className).toContain("top-1/2")
    expect(label.className).toContain("-translate-y-1/2")
  }
})

it("shows no tick row and no threshold description", () => {
  render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

  expect(screen.queryByText("0.5")).toBeNull()
  expect(screen.queryByText(/at or under the allow threshold/i)).toBeNull()
})

it("shows the two number inputs, named and valued to four decimals for the row they hold", () => {
  render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  const deny = screen.getByRole("spinbutton", { name: "Deny threshold" }) as HTMLInputElement
  expect(allow.value).toBe("0.2000")
  expect(deny.value).toBe("0.8000")
})

it("shows a zone-coloured dot beside each input's label", () => {
  render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

  const allowLabel = screen.getByText("Allow threshold").closest("label") as HTMLElement
  const denyLabel = screen.getByText("Deny threshold").closest("label") as HTMLElement

  const allowDot = allowLabel.querySelector(".bg-status-done") as HTMLElement
  const denyDot = denyLabel.querySelector(".bg-status-danger") as HTMLElement
  expect(allowDot).not.toBeNull()
  expect(allowDot.getAttribute("aria-hidden")).toBe("true")
  expect(denyDot).not.toBeNull()
  expect(denyDot.getAttribute("aria-hidden")).toBe("true")
})

it("puts each input directly under its own handle", () => {
  render(<ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />)

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  const deny = screen.getByRole("spinbutton", { name: "Deny threshold" }) as HTMLInputElement

  const allowColumn = allow.closest("div.absolute") as HTMLElement
  const denyColumn = deny.closest("div.absolute") as HTMLElement
  expect(allowColumn.style.left).toBe("20%")
  expect(denyColumn.style.left).toBe("80%")
})

it("formats an input to four decimals on first render, after a drag, and after a commit, but not while typing", () => {
  render(<ThresholdRange allowThreshold={0.3} denyThreshold={0.8} mutate={mutate} />)

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  expect(allow.value).toBe("0.3000")

  const allowInput = screen.getByRole("slider", { name: "Allow threshold" }) as HTMLElement
  // The accessible role sits on a visually hidden input; the draggable thumb
  // — the element `onPointerDown` is bound to — is its own parent div.
  const allowThumb = allowInput.parentElement as HTMLElement
  const track = allowThumb.closest('[data-slot="slider-track"]') as HTMLElement
  const control = track.parentElement as HTMLElement

  // jsdom lays nothing out: fake a 200px-wide track and a 10px-wide thumb,
  // so a pointer press and move against those fixed rects reads as a move
  // from the value 0.3 to 0.31.
  vi.spyOn(control, "getBoundingClientRect").mockReturnValue({
    left: 0,
    right: 200,
    top: 0,
    bottom: 20,
    width: 200,
    height: 20,
    x: 0,
    y: 0,
    toJSON: () => {},
  } as DOMRect)
  vi.spyOn(allowThumb, "getBoundingClientRect").mockReturnValue({
    left: 55,
    right: 65,
    top: 0,
    bottom: 20,
    width: 10,
    height: 10,
    x: 55,
    y: 0,
    toJSON: () => {},
  } as DOMRect)
  control.setPointerCapture = vi.fn()
  control.hasPointerCapture = vi.fn(() => false)
  control.releasePointerCapture = vi.fn()

  fireEvent.pointerDown(allowThumb, { clientX: 60, clientY: 10, button: 0, pointerId: 1 })
  // buttons: 1 tells the library the primary button is still down during the
  // move; without it the move reads like the button was released elsewhere.
  fireEvent.pointerMove(document, { clientX: 68, clientY: 10, pointerId: 1, buttons: 1 })
  fireEvent.pointerUp(document, { clientX: 68, clientY: 10, pointerId: 1 })
  expect(allow.value).toBe("0.3100")

  fireEvent.change(allow, { target: { value: "0.5" } })
  expect(allow.value).toBe("0.5")

  fireEvent.blur(allow)
  expect(allow.value).toBe("0.5000")
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
  expect(screen.getByText("0.4200")).toBeDefined()
})

function zoneLabels(container: HTMLElement): HTMLElement[] {
  return Array.from(container.querySelectorAll("span.pointer-events-none.absolute")).filter(
    (element) => element.className.includes("top-1/2"),
  ) as HTMLElement[]
}

it("positions zone labels at the center of each zone and updates them when thresholds move", () => {
  const { container, rerender } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )

  let labels = zoneLabels(container)
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

  labels = zoneLabels(container)
  expect(labels[0]?.style.left).toBe("15%")
  expect(labels[1]?.style.left).toBe("50%")
  expect(labels[2]?.style.left).toBe("85%")
})

it("hides only the narrow zone label", () => {
  const { container } = render(
    <ThresholdRange allowThreshold={0.45} denyThreshold={0.55} mutate={mutate} />,
  )

  const labels = zoneLabels(container)
  expect(labels.length).toBe(3)
  // Ask zone is 10% wide (< 15%), so only Ask is hidden
  expect(labels[0]?.className).toContain("opacity-100")
  expect(labels[1]?.className).toContain("opacity-0")
  expect(labels[2]?.className).toContain("opacity-100")
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
  render(
    <>
      <Toaster />
      <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />
    </>,
  )

  const allow = screen.getByRole("spinbutton", { name: "Allow threshold" }) as HTMLInputElement
  fireEvent.change(allow, { target: { value: "0.95" } })
  fireEvent.blur(allow)

  expect(
    await screen.findByText(/the allow threshold must stay under the deny threshold/),
  ).toBeDefined()
  await waitFor(() => expect(allow.value).toBe("0.2000"))
})
