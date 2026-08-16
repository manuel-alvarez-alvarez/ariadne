import { useParams } from "react-router-dom"

import { StubScreen } from "@/components/stub-screen"

export function TaskDetailPage() {
  const { taskId } = useParams<{ taskId: string }>()

  return (
    <StubScreen title={`Task ${taskId ?? ""}`} owner="src/features/tasks">
      One task: description, status history, its conversation, reviews per round and the branch
      diff.
    </StubScreen>
  )
}
