import { StubScreen } from "@/components/stub-screen"

export function SessionsListPage() {
  return (
    <StubScreen title="Sessions" owner="src/features/sessions">
      Lists <code className="font-mono">GET /v1/sessions</code>: every agent session with its role,
      profile, agent kind, tmux session and status.
    </StubScreen>
  )
}
