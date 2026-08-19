// @vitest-environment jsdom

/**
 * The guard itself, on a form small enough that nothing but the dismissal is
 * under test.
 *
 * Every way out of a dialog funnels through the root's `onOpenChange`, so what
 * has to hold is the fork it takes: pristine, the parent hears the close it
 * has always heard; dirty, it hears nothing until the question has been
 * answered, and answering it "keep editing" has to leave the form exactly as
 * it was rather than reopen it from scratch.
 */

import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { FormDialog } from "./form-dialog"
import { Button } from "./ui/button"
import { DialogClose, DialogContent, DialogFooter, DialogTitle } from "./ui/dialog"

/** A dialog whose dirtiness is whatever has been typed into its one field. */
function Harness({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  const [text, setText] = useState("")
  return (
    <FormDialog open onOpenChange={onOpenChange} dirty={text.length > 0}>
      <DialogContent>
        <DialogTitle>Edit thing</DialogTitle>
        <label htmlFor="harness-brief">Brief</label>
        <input id="harness-brief" value={text} onChange={(event) => setText(event.target.value)} />
        <DialogFooter>
          <DialogClose render={<Button type="button" />}>Cancel</DialogClose>
        </DialogFooter>
      </DialogContent>
    </FormDialog>
  )
}

afterEach(cleanup)

describe("dismissing a pristine form", () => {
  it("closes on Escape, with nothing to ask about", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<Harness onOpenChange={onOpenChange} />)

    await user.keyboard("{Escape}")

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })

  it("closes on the form's own Cancel just the same", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<Harness onOpenChange={onOpenChange} />)

    await user.click(screen.getByRole("button", { name: "Cancel" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.queryByText("Discard changes?")).toBeNull()
  })
})

describe("dismissing a form with unsaved input", () => {
  it("asks instead of closing, and keeps everything typed when the answer is no", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<Harness onOpenChange={onOpenChange} />)

    const field = screen.getByLabelText("Brief")
    await user.type(field, "a paragraph nobody wants to type twice")
    await user.keyboard("{Escape}")

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()

    await user.click(screen.getByRole("button", { name: "Keep editing" }))

    expect(screen.queryByText("Discard changes?")).toBeNull()
    expect(onOpenChange).not.toHaveBeenCalled()
    expect((screen.getByLabelText("Brief") as HTMLInputElement).value).toBe(
      "a paragraph nobody wants to type twice",
    )
  })

  it("closes once the discard is confirmed", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<Harness onOpenChange={onOpenChange} />)

    await user.type(screen.getByLabelText("Brief"), "typed")
    await user.click(screen.getByRole("button", { name: "Cancel" }))

    await user.click(await screen.findByRole("button", { name: "Discard" }))

    expect(onOpenChange).toHaveBeenCalledWith(false)
  })

  it("asks on an outside press too, which is the click that started all this", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<Harness onOpenChange={onOpenChange} />)

    await user.type(screen.getByLabelText("Brief"), "typed")
    await user.click(document.body)

    expect(await screen.findByText("Discard changes?")).toBeDefined()
    expect(onOpenChange).not.toHaveBeenCalled()
  })
})
