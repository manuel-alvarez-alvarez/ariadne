/**
 * How the identifiers on a task are shortened for the screen. Timestamps are
 * formatted by `@/lib/time` and errors described by `@/lib/errors`, which every
 * feature shares.
 */

/**
 * Ids are 26-character ULIDs: unreadable in full, but the tail is enough to
 * tell two of them apart. The full id always stays available as a `title`.
 */
export function shortId(id: string): string {
  return id.length <= 10 ? id : `…${id.slice(-8)}`
}

/** Git object ids are shown the way git shows them. */
export function shortSha(sha: string): string {
  return sha.slice(0, 10)
}
