/**
 * What "no repositories are registered" is called, wherever that has to be
 * said.
 *
 * It is said in two places — the repositories screen, which is empty, and the
 * new-goal dialog, which then has nothing to offer — and they said it
 * differently: "No repositories yet" against "No repositories registered",
 * each with a sentence of its own. Two wordings for one state read as two
 * states.
 *
 * The words are shared now; the way *out* of it is not, because it genuinely
 * differs. On the screen that registers repositories the way out is its form;
 * anywhere else it is a link to that screen. So the action is the caller's,
 * and only the caller's.
 */

import type { ReactNode } from "react"

import { EmptyState } from "@/components/empty-state"

export function NoRepositories({
  action,
  emphasis,
  className,
}: {
  /** The way out of it, which is whatever the surface can offer. */
  action: ReactNode
  /** `quiet` inside a dialog or a panel; the default where it leads a screen. */
  emphasis?: "prominent" | "quiet"
  className?: string
}) {
  return (
    <EmptyState
      emphasis={emphasis}
      className={className}
      title="No repositories registered"
      description="A goal is created against registered checkouts, so there is nothing to work in until one is registered. ariadne repo add does it from a terminal."
      action={action}
    />
  )
}
