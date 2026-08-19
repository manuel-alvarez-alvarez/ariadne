/**
 * How the ids the daemon hands out are shortened for the screen.
 *
 * App-wide, like `@/lib/time` and `@/lib/errors`: tasks, sessions and the
 * attention list all show the same id the same way, and the full value stays
 * one click away wherever {@link import("@/components/copyable-id").CopyableId}
 * renders it.
 */

/**
 * Ids are 26-character ULIDs: unreadable in full, but the tail is enough to
 * tell two of them apart. The full id always stays one hint or one copy away
 * wherever this is shown.
 */
export function shortId(id: string): string {
  return id.length <= 10 ? id : `…${id.slice(-8)}`
}
