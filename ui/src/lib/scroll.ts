/**
 * Finding the box a piece of the page actually scrolls in.
 *
 * A thread does not scroll itself: it grows, and the surface around it — the
 * side panel's popup, or the page when the thread is on one — is the single
 * scroll container. Which of the two it is depends on where the thread was
 * rendered, so it is looked up from the node rather than assumed, and both the
 * compose box (scrolling itself into view after a send) and the message list
 * (following the newest message) ask the same question of the same helper.
 */

/**
 * The nearest ancestor that scrolls, or `document.scrollingElement` when
 * nothing on the way up does and the page itself is the container.
 */
export function scrollParent(el: Element | null): Element | null {
  for (let node = el?.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    if (overflowY === "auto" || overflowY === "scroll") return node
  }
  return document.scrollingElement
}

/**
 * How far from the bottom still counts as "at the bottom", in px.
 *
 * Sub-pixel heights leave `scrollTop` a fraction short of the end, and a reader
 * a line away from the newest message is still reading the newest message: both
 * should keep following rather than being told there is something below.
 */
const AT_BOTTOM_SLACK = 48

/** Whether this container is showing the end of its content. */
export function isAtBottom(container: Element): boolean {
  return container.scrollHeight - container.clientHeight - container.scrollTop <= AT_BOTTOM_SLACK
}

/** Put this container's end on screen. */
export function scrollToBottom(container: Element, behavior: ScrollBehavior = "auto"): void {
  container.scrollTo({ top: container.scrollHeight, behavior })
}
