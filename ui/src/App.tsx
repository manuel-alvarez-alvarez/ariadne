/**
 * Provider stack, outermost first:
 *
 *   ThemeProvider        light/dark class on <html>
 *   QueryClientProvider  the cache every screen reads
 *   TooltipProvider      shared tooltip delay/portal
 *   EventStreamProvider  the single SSE connection feeding that cache
 *   RouterProvider       the shell and the routed screens
 */

import { QueryClientProvider } from "@tanstack/react-query"
import { useState } from "react"
import { RouterProvider } from "react-router-dom"

import { ThemeProvider } from "@/components/theme-provider"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { EventStreamProvider } from "@/events/provider"
import { createQueryClient } from "@/lib/query-client"
import { router } from "@/routes/router"

export default function App() {
  const [queryClient] = useState(createQueryClient)

  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <EventStreamProvider>
            <RouterProvider router={router} />
            <Toaster />
          </EventStreamProvider>
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  )
}
