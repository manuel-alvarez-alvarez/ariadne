// @vitest-environment jsdom

/**
 * The compose box, driven through a real `useMutation` rather than a faked
 * result object: what the tests pin — Send's disabled states, the chord, the
 * draft surviving a failure — are exactly the mutation lifecycle folded into
 * the box, and a hand-rolled `UseMutationResult` would assert the stub.
 * Only the daemon call itself is a stub each test writes.
 *
 * The addressee picker is driven the same way, through the real Base UI select:
 * what matters about it is what ends up in the posted `to`, and that is only
 * true of the select the user actually clicks.
 */

import { QueryClient, QueryClientProvider, useMutation } from "@tanstack/react-query"
import { cleanup, render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import type { CreateMessageRequest, MessageDto } from "@/api"

import { type Addressee, MessageComposer } from "./message-composer"

// `globals` is off, so nothing unmounts a screen between tests but this.
afterEach(cleanup)

// jsdom does not lay out or scroll; the box scrolls its panel after a send,
// and here the call only has to exist.
beforeEach(() => {
  Element.prototype.scrollTo = vi.fn()
})

const SENT: MessageDto = {
  id: "01msg",
  goal_id: "01goal",
  author_role: "user",
  body: "hello there",
  created_at: "2026-08-18T12:00:00Z",
}

type Send = (message: CreateMessageRequest) => Promise<MessageDto>

function Harness({ send, addressees }: { send: Send; addressees?: Addressee[] }) {
  const post = useMutation<MessageDto, Error, CreateMessageRequest>({ mutationFn: send })
  return (
    <MessageComposer
      post={post}
      label="Message the thread"
      placeholder="Say something"
      addressees={addressees}
    />
  )
}

function mountComposer(send: Send, addressees?: Addressee[]) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  const view = render(
    <QueryClientProvider client={client}>
      <Harness send={send} addressees={addressees} />
    </QueryClientProvider>,
  )
  return {
    box: screen.getByRole("textbox", { name: "Message the thread" }),
    button: screen.getByRole("button", { name: "Send" }) as HTMLButtonElement,
    /** Re-render with another set of addressees, as a task losing a reviewer does. */
    setAddressees: (next: Addressee[]) =>
      view.rerender(
        <QueryClientProvider client={client}>
          <Harness send={send} addressees={next} />
        </QueryClientProvider>,
      ),
  }
}

const ALICE: Addressee = { id: "01alice", name: "Alice" }
const BOB: Addressee = { id: "01bob", name: "Bob" }

/** Open the picker and choose one of its options by name. */
async function address(user: ReturnType<typeof userEvent.setup>, option: string) {
  await user.click(screen.getByRole("combobox", { name: "Addressee" }))
  await user.click(await screen.findByRole("option", { name: option }))
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
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box, button } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "  hello there  ")
  await user.click(button)

  expect(send).toHaveBeenCalledWith({ body: "hello there", to: undefined }, expect.anything())
  expect((box as HTMLTextAreaElement).value).toBe("")
})

it("sends on the modifier chord, with either modifier", async () => {
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "by keyboard")
  await user.keyboard("{Control>}{Enter}{/Control}")
  expect(send).toHaveBeenCalledWith({ body: "by keyboard", to: undefined }, expect.anything())

  await user.type(box, "again")
  await user.keyboard("{Meta>}{Enter}{/Meta}")
  expect(send).toHaveBeenCalledWith({ body: "again", to: undefined }, expect.anything())
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

it("has no picker when the thread has no one to address", () => {
  mountComposer(() => Promise.resolve(SENT))
  expect(screen.queryByRole("combobox", { name: "Addressee" })).toBeNull()
})

it("posts the addressee the picker was left on, and keeps it for the next message", async () => {
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box, button } = mountComposer(send, [ALICE, BOB])
  const user = userEvent.setup()

  await address(user, "Alice")
  await user.type(box, "over to you")
  await user.click(button)
  expect(send).toHaveBeenCalledWith({ body: "over to you", to: ALICE.id }, expect.anything())

  await user.type(box, "and one more thing")
  await user.click(button)
  expect(send).toHaveBeenCalledWith({ body: "and one more thing", to: ALICE.id }, expect.anything())
})

it("addresses the thread again once the picker is cleared", async () => {
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box, button } = mountComposer(send, [ALICE])
  const user = userEvent.setup()

  await address(user, "Alice")
  await address(user, "the thread")

  await user.type(box, "anyone")
  await user.click(button)

  expect(send).toHaveBeenCalledWith({ body: "anyone", to: undefined }, expect.anything())
})

it("drops an addressee that has left the thread", async () => {
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box, button, setAddressees } = mountComposer(send, [ALICE, BOB])
  const user = userEvent.setup()

  await address(user, "Alice")
  setAddressees([BOB])

  await user.type(box, "still going out")
  await user.click(button)

  expect(send).toHaveBeenCalledWith({ body: "still going out", to: undefined }, expect.anything())
})
