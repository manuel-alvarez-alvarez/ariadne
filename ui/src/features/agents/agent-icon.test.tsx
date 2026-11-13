// @vitest-environment jsdom

/**
 * The icon of one agent: which source it draws from, and what it falls back
 * to. The bundled SVG wins over the registry URL, and a URL that fails to
 * load leaves the first letter of the id rather than a broken image.
 */

import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { AgentIcon } from "./agent-icon"

const CDN = "https://cdn.agentclientprotocol.com/registry/v1/latest/goose.svg"

describe("AgentIcon", () => {
  it("draws the bundled SVG of an agent that has one, over the registry URL", () => {
    render(<AgentIcon agentId="codex-acp" icon={CDN} label="Codex" />)

    expect(screen.getByRole("img", { name: "Codex" }).getAttribute("src")).toBe(
      "/agent-icons/codex.svg",
    )
  })

  it("bundles an icon for Claude, both opencode ids and Gemini", () => {
    for (const id of ["claude-acp", "opencode", "opencode-v2", "gemini"]) {
      const { container, unmount } = render(<AgentIcon agentId={id} />)
      expect(container.querySelector("img")?.getAttribute("src")).toMatch(/^\/agent-icons\//)
      unmount()
    }
  })

  it("draws the registry URL of an agent with nothing bundled", () => {
    render(<AgentIcon agentId="goose" icon={CDN} label="Goose" />)

    expect(screen.getByRole("img", { name: "Goose" }).getAttribute("src")).toBe(CDN)
  })

  it("falls back to the first letter where there is no icon", () => {
    render(<AgentIcon agentId="goose" icon={null} />)

    expect(screen.getByText("g")).toBeTruthy()
  })

  it("falls back to the first letter when the image fails to load", () => {
    render(<AgentIcon agentId="goose" icon={CDN} label="Goose" />)

    fireEvent.error(screen.getByRole("img", { name: "Goose" }))

    expect(screen.getByRole("img", { name: "Goose" }).textContent).toBe("g")
    expect(document.querySelector("img")).toBeNull()
  })

  it("tries the registry URL when the bundled image fails, then the letter when it fails too", () => {
    render(<AgentIcon agentId="codex-acp" icon={CDN} label="Codex" />)

    fireEvent.error(screen.getByRole("img", { name: "Codex" }))
    expect(screen.getByRole("img", { name: "Codex" }).getAttribute("src")).toBe(CDN)

    fireEvent.error(screen.getByRole("img", { name: "Codex" }))
    expect(screen.getByRole("img", { name: "Codex" }).textContent).toBe("c")
    expect(document.querySelector("img")).toBeNull()
  })

  it("is decorative without a label, and sized as asked", () => {
    const { container } = render(<AgentIcon agentId="goose" size={32} />)

    const mark = container.firstElementChild as HTMLElement
    expect(mark.getAttribute("aria-hidden")).toBe("true")
    expect(mark.style.width).toBe("32px")
  })
})
