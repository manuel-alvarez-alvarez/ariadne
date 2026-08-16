import { StubScreen } from "@/components/stub-screen"

export function GoalsListPage() {
  return (
    <StubScreen title="Goals" owner="src/features/goals">
      Lists <code className="font-mono">GET /v1/goals</code> with status, repos, required approvals
      and task counts, and starts new goals.
    </StubScreen>
  )
}
