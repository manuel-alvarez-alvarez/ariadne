/**
 * The screen a crash lands on: the router's `errorElement`.
 *
 * It used to be {@link NotFoundPage}, which meant a component that threw was
 * announced as "Nothing here" — the words for a URL that does not exist —
 * followed by whatever `error.message` happened to say. The two are different
 * events with different next steps: a wrong address is corrected by going
 * somewhere that exists, a crash is survived by reloading and reported by
 * handing someone what it said.
 *
 * `isRouteErrorResponse` is what tells them apart. A router-thrown response is
 * a request that resolved to nothing, so it is the 404 by another route and
 * gets the 404's words; everything else got here by throwing, and gets the
 * stack behind a copy button.
 */

import { isRouteErrorResponse, Link, useRouteError } from "react-router-dom"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { copyText } from "@/lib/clipboard"
import { NotFoundPage } from "@/routes/not-found-page"
import { paths } from "@/routes/paths"

export function RouteErrorPage() {
  const error = useRouteError()
  if (isRouteErrorResponse(error) && error.status === 404) return <NotFoundPage />

  return (
    <div className="flex flex-col items-start gap-4 p-6">
      <div className="min-w-0">
        <h1 className="font-heading text-lg font-semibold">Something went wrong</h1>
        <p className="text-sm text-muted-foreground">
          The screen stopped rendering. Reloading the window starts it over — nothing the daemon
          holds was touched by this.
        </p>
      </div>

      {/* What it said, verbatim and unstyled: the message is for whoever ends
          up reading it, and rewording an exception helps nobody. */}
      <pre className="max-h-64 w-full overflow-auto rounded-lg border bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap">
        {errorSummary(error)}
      </pre>

      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={() => window.location.reload()}>Reload</Button>
        <Button
          variant="outline"
          onClick={() => {
            void copyText(errorDetails(error)).then((copied) =>
              copied
                ? toast.success("Error details copied")
                : toast.error("Could not copy the details"),
            )
          }}
        >
          Copy details
        </Button>
        <Button variant="ghost" render={<Link to={paths.goals()} />}>
          Back to goals
        </Button>
      </div>
    </div>
  )
}

/** The one line the screen shows: what was thrown, said as briefly as it can be. */
function errorSummary(error: unknown): string {
  if (isRouteErrorResponse(error)) return `${error.status} ${error.statusText}`.trim()
  if (error instanceof Error) return `${error.name}: ${error.message}`
  return String(error)
}

/**
 * The whole of it, for the clipboard: the stack where there is one, and the
 * address it happened at, which is the first thing anybody asks about a crash.
 */
function errorDetails(error: unknown): string {
  const where = typeof window === "undefined" ? "" : `\n\nat ${window.location.href}`
  if (isRouteErrorResponse(error)) {
    const body = typeof error.data === "string" ? error.data : JSON.stringify(error.data)
    return `${errorSummary(error)}${body ? `\n${body}` : ""}${where}`
  }
  if (error instanceof Error) return `${error.stack ?? errorSummary(error)}${where}`
  return `${String(error)}${where}`
}
