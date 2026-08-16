/**
 * The snapshot has to arrive at the terminal as one write that carries its own
 * reset. Anything else — a `reset()` call beside the write, or a reset written
 * separately — can be reordered against output the dropped connection had
 * already queued, and the stale output outlives the reset meant to remove it.
 */

import { describe, expect, it, vi } from "vitest"

import { SNAPSHOT_PREFIX, type TerminalWriter, writeDelta, writeSnapshot } from "./terminal-sink"

/** A terminal that records what it was told, including the reset it must not be told. */
function fakeTerminal() {
  return {
    write: vi.fn<TerminalWriter["write"]>(),
    reset: vi.fn(),
    clear: vi.fn(),
  }
}

describe("writeSnapshot", () => {
  it("resets in the stream rather than out of band", () => {
    const terminal = fakeTerminal()

    writeSnapshot(terminal, "hello")

    // One call: the reset must not be separable from its snapshot, or the
    // buffered writer can put queued output between them.
    expect(terminal.write.mock.calls).toEqual([[`${SNAPSHOT_PREFIX}hello`]])
    expect(terminal.reset).not.toHaveBeenCalled()
    expect(terminal.clear).not.toHaveBeenCalled()
  })

  it("cancels an unfinished sequence before the reset, not after", () => {
    // RIS itself starts with ESC, so a parser left mid-sequence by a truncated
    // chunk would eat it. CAN has to come first.
    expect(SNAPSHOT_PREFIX).toBe("\x18\x1bc")
  })
})

describe("writeDelta", () => {
  it("appends the chunk verbatim", () => {
    const terminal = fakeTerminal()

    writeDelta(terminal, "\x1b[32mok\x1b[0m\r\n")

    expect(terminal.write.mock.calls).toEqual([["\x1b[32mok\x1b[0m\r\n"]])
    expect(terminal.reset).not.toHaveBeenCalled()
  })
})
