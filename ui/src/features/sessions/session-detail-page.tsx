import { useParams } from "react-router-dom"

import { StubScreen } from "@/components/stub-screen"

export function SessionDetailPage() {
  const { sessionId } = useParams<{ sessionId: string }>()

  return (
    <StubScreen title={`Session ${sessionId ?? ""}`} owner="src/features/sessions">
      One agent session: its tmux pane output, the raw agent events it reported, and the kill/resume
      controls.
    </StubScreen>
  )
}
