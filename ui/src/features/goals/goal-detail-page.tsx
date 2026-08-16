import { useParams } from "react-router-dom"

import { StubScreen } from "@/components/stub-screen"

export function GoalDetailPage() {
  const { goalId } = useParams<{ goalId: string }>()

  return (
    <StubScreen title={`Goal ${goalId ?? ""}`} owner="src/features/goals">
      One goal: the planner thread, its repos, and the board of its tasks grouped by status. Task
      cards link to <code className="font-mono">/tasks/:taskId</code>, which
      <code className="font-mono"> src/features/tasks</code> owns.
    </StubScreen>
  )
}
