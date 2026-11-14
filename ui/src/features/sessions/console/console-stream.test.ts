/**
 * The console stream client, driven from the server's side of an
 * `EventSource` stand-in: a snapshot and the events after it reach their
 * callbacks, and a `resync` or a drop is retried on the backoff.
 */

import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { FakeEventSource, latestSource, stubEventSource } from "@/test/event-source"

import { ConsoleStream, consoleStreamUrl } from "./console-stream"

const SESSION_ID = "01JSESS0000000000000000001"

beforeEach(() => {
  stubEventSource()
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

function open() {
  const handlers = { onSnapshot: vi.fn(), onEvent: vi.fn(), onStatus: vi.fn() }
  const stream = new ConsoleStream(consoleStreamUrl("http://127.0.0.1:7676/", SESSION_ID), handlers)
  stream.start()
  return { stream, ...handlers }
}

const EVENT = {
  id: "01EVENT0000000000000000001",
  kind: "agent_message",
  payload: { text: "hi" },
  summary: "hi",
  created_at: "2026-11-14T10:00:00Z",
}

it("reads the console stream of the session", () => {
  open()

  expect(latestSource().url).toBe(`http://127.0.0.1:7676/v1/sessions/${SESSION_ID}/console/stream`)
})

it("hands over the snapshot and every event after it", () => {
  const { onSnapshot, onEvent, onStatus } = open()
  const source = latestSource()

  source.succeed()
  source.emit("snapshot", [EVENT])
  source.emit("event", { ...EVENT, id: "01EVENT0000000000000000002" })

  expect(onStatus).toHaveBeenLastCalledWith("live", null)
  expect(onSnapshot).toHaveBeenCalledWith([EVENT])
  expect(onEvent).toHaveBeenCalledWith({ ...EVENT, id: "01EVENT0000000000000000002" })
})

it("reconnects on the backoff after a resync", () => {
  const { onStatus } = open()
  const source = latestSource()
  source.succeed()

  source.emit("resync", { missed: 3 })

  expect(source.closed).toBe(true)
  expect(onStatus).toHaveBeenLastCalledWith("reconnecting", "resync")
  expect(FakeEventSource.instances).toHaveLength(1)
  vi.advanceTimersByTime(500)
  expect(FakeEventSource.instances).toHaveLength(2)
})

it("reconnects on the backoff after a drop, and stops for good on stop", () => {
  const { stream, onStatus } = open()
  latestSource().succeed()

  latestSource().fail()

  expect(onStatus).toHaveBeenLastCalledWith("reconnecting", "console stream disconnected")
  vi.advanceTimersByTime(500)
  expect(FakeEventSource.instances).toHaveLength(2)

  stream.stop()
  expect(latestSource().closed).toBe(true)
})
