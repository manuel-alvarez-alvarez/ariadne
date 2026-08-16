import { Link, useRouteError } from "react-router-dom"

import { Button } from "@/components/ui/button"
import { paths } from "@/routes/paths"

export function NotFoundPage() {
  // Doubles as the router's `errorElement`, where a real error is available.
  const error = useRouteError()

  return (
    <div className="flex flex-col items-start gap-4 p-6">
      <div>
        <h1 className="font-heading text-lg font-semibold">Nothing here</h1>
        <p className="text-sm text-muted-foreground">
          {error instanceof Error ? error.message : "This page does not exist."}
        </p>
      </div>
      <Button render={<Link to={paths.goals()} />}>Back to goals</Button>
    </div>
  )
}
