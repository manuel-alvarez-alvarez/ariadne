/**
 * The address that leads nowhere: the catch-all route, and the words a
 * router-thrown 404 borrows (see `error-page.tsx`).
 *
 * It is only that now. It used to double as the router's `errorElement`, where
 * it announced a crashing component as "Nothing here" and printed the raw
 * exception under it — a screen with no reload, no way to report what happened,
 * and the wrong heading over it.
 */

import { Link } from "react-router-dom"

import { Button } from "@/components/ui/button"
import { paths } from "@/routes/paths"

export function NotFoundPage() {
  return (
    <div className="flex flex-col items-start gap-4 p-6">
      <div>
        <h1 className="font-heading text-lg font-semibold">Nothing here</h1>
        <p className="text-sm text-muted-foreground">This page does not exist.</p>
      </div>
      <Button render={<Link to={paths.goals()} />}>Back to goals</Button>
    </div>
  )
}
