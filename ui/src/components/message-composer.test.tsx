// @vitest-environment jsdom

/**
 * The compose box, driven through a real `useMutation` rather than a faked
 * result object: what the tests pin — Send's disabled states, the chord, the
 * draft surviving a failure — are exactly the mutation lifecycle folded into
 * the box, and a hand-rolled `UseMutationResult` would assert the stub.
 * Only the daemon call itself is a stub each test writes.
 */

import { QueryClient, QueryClientProvider, useMutation } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { MessageDto } from "@/api"

import { MessageComposer } from "./message-composer"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

// jsdom has no `scrollIntoView`; the box calls it on the sent message's
// anchor, and here it only has to exist.
beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
})

const SENT: MessageDto = {
  id: "01msg",
  goal_id: "01goal",
  author_role: "user",
  body: "hello there",
  created_at: "2026-08-18T12:00:00Z",
}

function Harness({ send }: { send: (body: string) => Promise<MessageDto> }) {
  const post = useMutation<MessageDto, Error, string>({ mutationFn: send })
  return <MessageComposer post={post} label="Message the thread" placeholder="Say something" />
}

function mountComposer(send: (body: string) => Promise<MessageDto>) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <Harness send={send} />
    </QueryClientProvider>,
  )
  return {
    box: screen.getByRole("textbox", { name: "Message the thread" }),
    button: screen.getByRole("button", { name: "Send" }) as HTMLButtonElement,
  }
}

it("disables Send until there is something other than whitespace to send", async () => {
  const { box, button } = mountComposer(() => Promise.resolve(SENT))
  const user = userEvent.setup()

  expect(button.disabled).toBe(true)
  await user.type(box, "   ")
  expect(button.disabled).toBe(true)
  await user.type(box, "hello")
  expect(button.disabled).toBe(false)
})

it("sends the trimmed draft and clears the box", async () => {
  const send = vi.fn((body: string) => Promise.resolve({ ...SENT, body }))
  const { box, button } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "  hello there  ")
  await user.click(button)

  expect(send).toHaveBeenCalledWith("hello there", expect.anything())
  expect((box as HTMLTextAreaElement).value).toBe("")
})

it("sends on the modifier chord, with either modifier", async () => {
  const send = vi.fn((body: string) => Promise.resolve({ ...SENT, body }))
  const { box } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "by keyboard")
  await user.keyboard("{Control>}{Enter}{/Control}")
  expect(send).toHaveBeenCalledWith("by keyboard", expect.anything())

  await user.type(box, "again")
  await user.keyboard("{Meta>}{Enter}{/Meta}")
  expect(send).toHaveBeenCalledWith("again", expect.anything())
})

it("keeps the draft and shows the error on failure, cleared by the next edit", async () => {
  const { box, button } = mountComposer(() => Promise.reject(new Error("daemon said no")))
  const user = userEvent.setup()

  await user.type(box, "doomed")
  await user.click(button)

  expect((box as HTMLTextAreaElement).value).toBe("doomed")
  expect(screen.getByText("Could not send the message")).toBeTruthy()
  expect(screen.getByText("daemon said no")).toBeTruthy()

  await user.type(box, "!")
  expect(screen.queryByText("Could not send the message")).toBeNull()
})

it("disables Send while the post is in flight and does not double-send", async () => {
  const send = vi.fn(() => new Promise<MessageDto>(() => {}))
  const { box, button } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "slow one")
  await user.click(button)

  expect(button.disabled).toBe(true)
  await user.keyboard("{Control>}{Enter}{/Control}")
  expect(send).toHaveBeenCalledTimes(1)
})
