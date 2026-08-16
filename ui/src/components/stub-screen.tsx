/**
 * Placeholder for a screen that the scaffold only wires up — the feature tasks
 * implement the real thing.
 *
 * Scaffolding only: once every feature has replaced its stub, this file goes
 * away with the last usage.
 */

import { ConstructionIcon } from "lucide-react"
import type { ReactNode } from "react"

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"

export function StubScreen({
  title,
  owner,
  children,
}: {
  title: string
  /** Which feature directory owns this screen. */
  owner: string
  /** What the finished screen is meant to show. */
  children?: ReactNode
}) {
  return (
    <Card className="max-w-2xl">
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ConstructionIcon className="size-4 text-muted-foreground" />
          {title}
        </CardTitle>
        <CardDescription>
          Not implemented yet — this screen belongs to{" "}
          <code className="font-mono text-xs">{owner}</code>.
        </CardDescription>
      </CardHeader>
      {children ? (
        <CardContent className="text-sm text-muted-foreground">{children}</CardContent>
      ) : null}
    </Card>
  )
}
