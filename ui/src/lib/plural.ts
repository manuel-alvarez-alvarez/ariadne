/**
 * "1 task", "3 tasks" — the count and its noun, agreeing.
 *
 * Five screens were each spelling this out inline as
 * `{n} {n === 1 ? "task" : "tasks"}`, which is a lot of ternary for a rule
 * that never varies. The regular `-s` plural is the default because almost
 * every noun this app counts takes it: tasks, items, verdicts, profiles. The
 * one that does not — "repositories" — passes its plural in.
 */

export function plural(count: number, noun: string, plural = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : plural}`
}
