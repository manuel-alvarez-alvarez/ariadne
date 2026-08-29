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
 *
 * The draft is the box's other half, and it is deliberately *not* the box's
 * state: it is written to session storage as it is typed, so what a test
 * unmounts and mounts again is what a user closing and reopening a panel gets.
 * The storage is the real one the app uses (a shim from `@/test/setup`), and
 * each test starts from an empty one.
 */

import { QueryClient, QueryClientProvider, useMutation } from "@tanstack/react-query"
import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, expect, it, vi } from "vitest"

import type { CreateMessageRequest, MessageDto } from "@/api"
import { aMessage } from "@/test/fixtures"
import { type Addressee, MessageComposer } from "./message-composer"

// `globals` is off, so nothing unmounts a screen between tests but this.

// Storage outlives a test the way it outlives a panel, and a draft one test
// left behind is the next one's compose box already full.
beforeEach(() => {
  sessionStorage.clear()
})

const SENT: MessageDto = aMessage({ body: "hello there" })

type Send = (message: CreateMessageRequest) => Promise<MessageDto>

const THREAD = "task:01TASK" as const

function Harness({
  send,
  addressees,
  closedHint,
}: {
  send: Send
  addressees?: Addressee[]
  closedHint?: string
}) {
  const post = useMutation<MessageDto, Error, CreateMessageRequest>({ mutationFn: send })
  return (
    <MessageComposer
      post={post}
      draftKey={THREAD}
      label="Message the thread"
      placeholder="Say something"
      addressees={addressees}
      closedHint={closedHint}
    />
  )
}

function mountComposer(send: Send, addressees?: Addressee[], closedHint?: string) {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  const wrap = (next?: Addressee[]) => (
    <QueryClientProvider client={client}>
      <Harness send={send} addressees={next ?? addressees} closedHint={closedHint} />
    </QueryClientProvider>
  )
  const view = render(wrap())
  return {
    box: screen.getByRole("textbox", { name: "Message the thread" }) as HTMLTextAreaElement,
    button: screen.getByRole("button", { name: "Send" }) as HTMLButtonElement,
    /** Re-render with another set of addressees, as a task losing a reviewer does. */
    setAddressees: (next: Addressee[]) => view.rerender(wrap(next)),
    /** Close the panel and open it again, which is a fresh box on the same thread. */
    reopen: () => {
      view.unmount()
      render(wrap())
      return screen.getByRole("textbox", { name: "Message the thread" }) as HTMLTextAreaElement
    },
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

it("keeps the draft where reopening the thread finds it again", async () => {
  const { box, reopen } = mountComposer(() => Promise.resolve(SENT))
  const user = userEvent.setup()

  await user.type(box, "half a thought")
  // The panel is dismissed — an outside press, Escape, a link out — and opened
  // again on the same thread.
  expect(reopen().value).toBe("half a thought")
})

it("takes the draft with the message it sent", async () => {
  const send = vi.fn(({ body }: CreateMessageRequest) => Promise.resolve({ ...SENT, body }))
  const { box, button, reopen } = mountComposer(send)
  const user = userEvent.setup()

  await user.type(box, "off it goes")
  await user.click(button)
  expect(send).toHaveBeenCalledWith({ body: "off it goes", to: undefined }, expect.anything())

  // Sent is not unsent: the box that comes back is empty.
  expect(reopen().value).toBe("")
})

it("keeps a draft the daemon refused, for the panel that reopens on it", async () => {
  const { box, button, reopen } = mountComposer(() => Promise.reject(new Error("daemon said no")))
  const user = userEvent.setup()

  await user.type(box, "worth another try")
  await user.click(button)

  expect(reopen().value).toBe("worth another try")
})

it("closes the box on a thread nothing is working any more", () => {
  const send = vi.fn(() => Promise.resolve(SENT))
  const { box, button } = mountComposer(send, [ALICE], "Merged: no agent is left to read this.")

  expect(box.disabled).toBe(true)
  expect(button.disabled).toBe(true)
  // The hint takes the place of the send chord, and there is nobody to address.
  expect(screen.getByText("Merged: no agent is left to read this.")).toBeTruthy()
  expect(screen.queryByRole("combobox", { name: "Addressee" })).toBeNull()
})
