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

it("draws no danger marker where none is set, and one labelled with its value where it is", () => {
  const { rerender } = render(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} mutate={mutate} />,
  )
  expect(screen.queryByLabelText(/^Danger/)).toBeNull()

  rerender(
    <ThresholdRange allowThreshold={0.2} denyThreshold={0.8} danger={0.42} mutate={mutate} />,
  )
  expect(screen.getByLabelText("Danger 0.42")).toBeDefined()
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
