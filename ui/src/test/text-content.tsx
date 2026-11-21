/**
 * Matching text a component builds from several elements: "Could not load
 * goals on" and a `HostLabel` are two nodes, so `getByText` of the sentence
 * finds neither.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render } from "@testing-library/react"
import type { ReactNode } from "react"

const squash = (text: string | null) => (text ?? "").replace(/\s+/g, " ").trim()

/** A `getByText` matcher for the innermost element whose whole text matches. */
export function byContent(match: string | RegExp) {
  const hit = (text: string) => (typeof match === "string" ? text === match : match.test(text))
  return (_: string, element: Element | null): boolean =>
    element !== null &&
    hit(squash(element.textContent)) &&
    !Array.from(element.children).some((child) => hit(squash(child.textContent)))
}

/** The text a node reads as once drawn, for a toast the test holds as a value. */
export function contentOf(node: unknown): string {
  const view = render(
    <QueryClientProvider client={new QueryClient()}>{node as ReactNode}</QueryClientProvider>,
  )
  const text = squash(view.container.textContent)
  view.unmount()
  return text
}
