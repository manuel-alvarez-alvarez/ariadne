/**
 * Opening the xterm emulator a session's pane is drawn into, and everything
 * that has to be torn down with it.
 *
 * A plain function rather than a hook: the caller owns when it runs and what it
 * keeps, and nothing here is React's. What is here is the emulator's
 * configuration, the GPU renderer it is given where there is one, and the three
 * things it has to be listened to for.
 *
 * Drawing goes through the GPU where there is one: `@xterm/addon-webgl` is
 * fetched once the emulator is open and loaded into it, and the DOM renderer
 * xterm starts with is what stays when it cannot be — no WebGL2 in this
 * browser, no addon delivered, or a context the driver takes back later.
 */

import { FitAddon } from "@xterm/addon-fit"
import type { WebglAddon } from "@xterm/addon-webgl"
import { Terminal } from "@xterm/xterm"
import "@xterm/xterm/css/xterm.css"

import type { PaneSize } from "./log-stream"
import { BASE_FONT_SIZE, LINE_HEIGHT } from "./pane-fit"
import { currentTerminalTheme } from "./terminal-chrome"

/** Lines kept above the viewport. A busy agent fills a pane fast. */
const SCROLLBACK = 5_000

/**
 * What to draw at until the daemon says otherwise: tmux's own default size,
 * which is what these panes are created at. A session that is already over has
 * no pane left to measure, and its console log was written at whatever size the
 * pane had then.
 */
const DEFAULT_PANE_SIZE: PaneSize = { cols: 80, rows: 24 }

interface OpenTerminal {
  terminal: Terminal
  /** Loaded for its measurements alone; the grid stays the pane's, so `fit()` is never called. */
  fit: FitAddon
  /** xterm's own screen, written by `open`: the element a row's cost is measured on. */
  screen: HTMLElement | null
  dispose: () => void
}

export function openTerminal(
  container: HTMLDivElement,
  handlers: {
    /** The viewport moved; `following` says whether it is on the newest output. */
    onFollowingChange: (following: boolean) => void
    /** The pane's grid changed, so the frame has to be fitted to it again. */
    onResize: () => void
  },
): OpenTerminal {
  const terminal = new Terminal({
    // tmux's captured pane ends its lines with a bare newline; without this
    // every line would start where the previous one stopped.
    convertEol: true,
    // Never fitted to the box: the stream is what says how big the grid is.
    cols: DEFAULT_PANE_SIZE.cols,
    rows: DEFAULT_PANE_SIZE.rows,
    // Turned on by the caller once the session is known to be live; a terminal
    // that cannot reach a pane must not pretend to.
    disableStdin: true,
    cursorBlink: false,
    cursorInactiveStyle: "none",
    scrollback: SCROLLBACK,
    fontFamily: "'Geist Mono Variable', ui-monospace, monospace",
    fontSize: BASE_FONT_SIZE,
    lineHeight: LINE_HEIGHT,
    allowTransparency: false,
    theme: currentTerminalTheme(),
  })
  const fit = new FitAddon()
  terminal.loadAddon(fit)
  terminal.open(container)

  // The GPU renderer, when this browser has one to give. Fetched on demand
  // rather than imported with the module, so a browser that cannot use it never
  // downloads it — and the DOM renderer is already drawing by the time it
  // lands, which is what makes every way of not getting one a fallback and not
  // a failure.
  let webgl: WebglAddon | null = null
  let disposed = false
  void (async () => {
    if (!supportsWebgl()) return
    try {
      const { WebglAddon } = await import("@xterm/addon-webgl")
      if (disposed) return
      const addon = new WebglAddon()
      terminal.loadAddon(addon)
      webgl = addon
      // The context can be taken back at any time — another tab exhausting the
      // GPU, a driver reset — and xterm draws through the DOM again as soon as
      // the addon is disposed. Which is the whole recovery: a new addon would
      // ask the same driver for the same context.
      addon.onContextLoss(() => {
        webgl = null
        addon.dispose()
      })
    } catch {
      // No WebGL2 context for the addon, or no addon: the renderer xterm opened
      // with is still drawing, and that is the fallback.
    }
  })()

  // Scrolled up into the history, the viewer stops seeing what the agent is
  // doing now; the frame offers a way back rather than leaving them there.
  const scrolled = terminal.onScroll(() => {
    const buffer = terminal.buffer.active
    handlers.onFollowingChange(buffer.viewportY >= buffer.baseY)
  })

  // The grid arrives in the stream and applies whenever the parser reaches it,
  // so the emulator itself — not the message that caused it — is what says a
  // new one is in effect and the terminal has to be fitted to it.
  //
  // Fitted, and not merely scaled: a grid change is also the moment the
  // emulator's cells may settle on a size the last fit did not measure — a
  // webfont that landed after the terminal opened is the usual way — and a fit
  // measured against the wrong cell height asked for a grid that does not fit
  // the frame. Measuring again cannot run away with itself, because a
  // measurement that has not changed is not a request.
  const resized = terminal.onResize(() => handlers.onResize())

  // xterm focuses itself when its own screen is clicked; this covers the
  // padding around it, so the whole box is somewhere to start typing. Keyboard
  // users need no equivalent — xterm's textarea is in the tab order — which is
  // why it is a DOM listener and not an `onClick` prop.
  const focusTerminal = () => terminal.focus()
  container.addEventListener("click", focusTerminal)

  return {
    terminal,
    fit,
    screen: container.querySelector<HTMLElement>(".xterm-screen"),
    dispose: () => {
      disposed = true
      container.removeEventListener("click", focusTerminal)
      scrolled.dispose()
      resized.dispose()
      // Before the terminal, which is what the addon draws for.
      webgl?.dispose()
      terminal.dispose()
    },
  }
}

/**
 * Whether this browser can hand xterm a GPU renderer at all.
 *
 * Asked before the addon is fetched, so a browser that cannot run it never
 * downloads it. The probe's context is given straight back: contexts are a
 * scarce per-page resource, and one held open for a yes-or-no answer is one the
 * renderer itself may then not get.
 */
function supportsWebgl(): boolean {
  try {
    const context = document.createElement("canvas").getContext("webgl2")
    if (!context) return false
    context.getExtension("WEBGL_lose_context")?.loseContext()
    return true
  } catch {
    return false
  }
}
