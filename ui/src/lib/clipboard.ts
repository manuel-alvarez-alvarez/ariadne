/**
 * Putting a string on the system clipboard, from wherever the app is running.
 *
 * `navigator.clipboard` is the modern path and the only one worth using in a
 * browser tab, but it exists solely in a secure context: the daemon is often
 * reached over plain http on a LAN address, and inside the Tauri webview the
 * API is either missing or rejects outright. A hidden textarea driven by the
 * deprecated `execCommand("copy")` stands behind it — it is what every webview
 * still honours, as long as it runs inside the click that asked for it, which
 * is why nothing is awaited before the fallback.
 */

/** True when the text made it to the clipboard, by either route. */
export async function copyText(text: string): Promise<boolean> {
  if (typeof navigator !== "undefined" && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // Denied, or no secure context. The textarea below is the second chance.
    }
  }
  return copyViaTextarea(text)
}

function copyViaTextarea(text: string): boolean {
  if (typeof document === "undefined") return false

  const textarea = document.createElement("textarea")
  textarea.value = text
  // `execCommand` copies the document's selection, so the node has to be in
  // the page and selectable — but invisible, and fixed so selecting it cannot
  // scroll whatever the user was looking at.
  textarea.setAttribute("readonly", "")
  textarea.setAttribute("aria-hidden", "true")
  textarea.style.position = "fixed"
  textarea.style.top = "0"
  textarea.style.left = "0"
  textarea.style.opacity = "0"
  document.body.append(textarea)

  try {
    textarea.select()
    return document.execCommand("copy")
  } catch {
    return false
  } finally {
    textarea.remove()
  }
}
