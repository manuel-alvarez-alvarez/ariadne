/**
 * The skill names in a comma-separated box, in the order they were written and
 * without the blanks or the repeats.
 *
 * Shared by the control that reads the box and the form values that send it,
 * so what the screen says an agent knows and what the daemon is asked for
 * cannot come apart.
 */
export function parseSkillNames(typed: string): string[] {
  return [
    ...new Set(
      typed
        .split(",")
        .map((name) => name.trim())
        .filter(Boolean),
    ),
  ]
}
