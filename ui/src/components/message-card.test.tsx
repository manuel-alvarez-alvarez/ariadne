// @vitest-environment jsdom

/**
 * The card, for the one thing it draws conditionally: the addressee pill. A
 * message with no recipient is the overwhelming majority of a thread and has
 * to keep the header it always had, so the test pins the absence too.
 */

import { render, screen } from "@testing-library/react"
import { expect, it } from "vitest"

import type { MessageDto } from "@/api"
import { aMessage } from "@/test/fixtures"
import { MessageCard } from "./message-card"

// `globals` is off, so nothing unmounts a screen between tests but this.

const MESSAGE: MessageDto = aMessage({ body: "have a look at this" })

it("shows no addressee pill on a message addressed to the thread", () => {
  render(<MessageCard message={MESSAGE} />)

  expect(screen.getByText("You")).toBeTruthy()
  expect(screen.queryByText(/→/)).toBeNull()
})

it("names the addressed profile", () => {
  render(
    <MessageCard
      message={{
        ...MESSAGE,
        recipient: { kind: "profile", profile_id: "01alice", profile_name: "Alice" },
      }}
    />,
  )

  expect(screen.getByText("→ Alice")).toBeTruthy()
})

it("falls back to the id of a profile that is gone", () => {
  render(
    <MessageCard message={{ ...MESSAGE, recipient: { kind: "profile", profile_id: "01alice" } }} />,
  )

  expect(screen.getByText("→ 01alice")).toBeTruthy()
})

it("addresses the user by the same word the author pill uses", () => {
  render(
    <MessageCard message={{ ...MESSAGE, author_role: "reviewer", recipient: { kind: "user" } }} />,
  )

  expect(screen.getByText("→ You")).toBeTruthy()
})
