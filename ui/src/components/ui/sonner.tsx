"use client"

/**
 * Where errors go, app-wide:
 *
 * - A mutation confirmed in a dialog reports its failure *in that dialog* — the
 *   dialog stays open, the daemon's envelope is rendered inline, and the user
 *   can read it next to what they were about to do. Everything built on
 *   `ConfirmDialog` and the two forms works this way, kill included: it asks
 *   first, so it has somewhere to answer.
 * - A row-level action fired without a dialog has nowhere inline to put an
 *   error, so it toasts. Resume is the one of these.
 *
 * Success is always a toast, whichever kind it was: there is nothing left on
 * screen to say it in.
 *
 * Toasts land top-right: near the header and the row actions that raise them,
 * rather than in the opposite corner of the window from every trigger. They
 * carry a close button because an error is read at the reader's pace, and an
 * explicit duration so "the daemon refused" is not gone in three seconds.
 */

import {
  CircleCheckIcon,
  InfoIcon,
  Loader2Icon,
  OctagonXIcon,
  TriangleAlertIcon,
} from "lucide-react"
import { useTheme } from "next-themes"
import { Toaster as Sonner, type ToasterProps } from "sonner"

/** Long enough to read a daemon message, short enough not to stack up. */
const TOAST_DURATION_MS = 6000

const Toaster = ({ ...props }: ToasterProps) => {
  const { theme = "system" } = useTheme()

  return (
    <Sonner
      theme={theme as ToasterProps["theme"]}
      className="toaster group"
      position="top-right"
      closeButton
      duration={TOAST_DURATION_MS}
      icons={{
        success: <CircleCheckIcon className="size-4" />,
        info: <InfoIcon className="size-4" />,
        warning: <TriangleAlertIcon className="size-4" />,
        error: <OctagonXIcon className="size-4" />,
        loading: <Loader2Icon className="size-4 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
