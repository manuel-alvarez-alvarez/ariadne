/**
 * The mount point of the status row under the composer. It shows the
 * session's status, the running turn and its clock, and the token totals. A
 * later task adds the session's state here.
 */

import type { SessionStatus } from "@/api"

import { StatusRow } from "../console/status"
import type { TokenTotals } from "../console/transcript"

export function TrajectoryStatus({
  status,
  turn,
  waiting,
  tokens,
}: {
  status: SessionStatus
  turn?: { tool?: string; seconds?: number }
  waiting: boolean
  tokens: TokenTotals
}) {
  return <StatusRow status={status} turn={turn} waiting={waiting} tokens={tokens} />
}
