import { useParams } from "react-router-dom"

import { StubScreen } from "@/components/stub-screen"
import { TaskBoard } from "@/features/tasks"

export function GoalDetailPage() {
  const { goalId } = useParams<{ goalId: string }>()

  return (
    <div className="flex flex-col gap-6">
      <StubScreen title={`Goal ${goalId ?? ""}`} owner="src/features/goals">
        One goal: the planner thread and its repos. The board below is the region reserved for
        <code className="font-mono"> src/features/tasks</code>.
      </StubScreen>
      {goalId ? <TaskBoard goalId={goalId} /> : null}
    </div>
  )
}
