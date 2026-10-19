import { it, type TestContext } from "vitest"

// Two retries give a confirmed intermittent test at most three attempts.
export function flaky(name: string, fn: (context: TestContext) => void | Promise<void>) {
  it(name, { retry: 2 }, fn)
}
