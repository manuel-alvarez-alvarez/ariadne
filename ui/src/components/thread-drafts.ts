/**
 * The unsent half of a conversation, kept where a mis-click cannot take it.
 *
 * A compose box holds the longest text the app asks for outside a form dialog,
 * and it sits in a side panel that closes on Escape and on any press outside
 * it. Typed into React state, three paragraphs to a reviewer are gone the
 * moment the panel is dismissed — and dismissing it is also how the user gets
 * at the task the message is about.
 *
 * So the draft lives in `sessionStorage`, one entry per thread, written on
 * every edit and deleted the moment the message is actually posted. Session
 * storage rather than local: a draft is something being written *now*, and one
 * that outlived a browser restart would come back as a surprise long after the
 * thread moved on. It is per tab for the same reason.
 *
 * Nothing here is reactive. The composer reads its draft once, when it mounts,
 * and owns it from then on; the panels read one only at the instant a
 * dismissal has to be judged (see `panel-sheet.tsx`). A store, a subscription
 * and a re-render per keystroke would buy neither of them anything.
 *
 * A browser with no storage to give — private mode, a webview with it turned
 * off — falls back to a map that lives as long as the page: the draft still
 * survives the panel closing and reopening, which is what it is for, and only
 * a reload takes it.
 */

/** Which thread a draft belongs to: `goal:01J…`, `task:01J…`. */
export type ThreadKey = `goal:${string}` | `task:${string}`

const PREFIX = "ariadne.draft."

/** Where a draft goes when `sessionStorage` refuses to hold it. */
const fallback = new Map<ThreadKey, string>()

/** What is in this thread's compose box, or `""` when nothing is. */
export function readDraft(key: ThreadKey): string {
  try {
    return sessionStorage.getItem(PREFIX + key) ?? ""
  } catch {
    return fallback.get(key) ?? ""
  }
}

/** Keep this thread's draft. Emptying it removes the entry rather than storing `""`. */
export function writeDraft(key: ThreadKey, draft: string): void {
  try {
    if (draft) sessionStorage.setItem(PREFIX + key, draft)
    else sessionStorage.removeItem(PREFIX + key)
  } catch {
    if (draft) fallback.set(key, draft)
    else fallback.delete(key)
  }
}
