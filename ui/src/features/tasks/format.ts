/**
 * How the git identifiers on a task are shortened for the screen. Ulids are
 * shortened by `@/lib/ids`, timestamps formatted by `@/lib/time` and errors
 * described by `@/lib/errors`, which every feature shares.
 */

/** Git object ids are shown the way git shows them. */
export function shortSha(sha: string): string {
  return sha.slice(0, 10)
}
