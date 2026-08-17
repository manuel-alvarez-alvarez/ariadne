/**
 * "1 task", "3 tasks" — the count and its noun, agreeing.
 *
 * Five screens were each spelling this out inline as
 * `{n} {n === 1 ? "task" : "tasks"}`, which is a lot of ternary for a rule
 * that never varies. Only the regular `-s` plural is handled, because every
 * noun this app counts takes it: tasks, items, verdicts, profiles. An
 * irregular one would need a second form passed in, and there is none yet.
 */

export function plural(count: number, noun: string): string {
  return `${count} ${count === 1 ? noun : `${noun}s`}`
}
