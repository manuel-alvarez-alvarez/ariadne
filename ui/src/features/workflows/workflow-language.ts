import { StreamLanguage, type StreamParser } from "@codemirror/language"

interface WorkflowState {
  lineStart: boolean
  metadataValue: boolean
  workflowName: boolean
}

const parser: StreamParser<WorkflowState> = {
  startState: () => ({ lineStart: true, metadataValue: false, workflowName: false }),
  token(stream, state) {
    if (stream.sol()) {
      state.lineStart = true
      state.metadataValue = false
    }
    if (stream.eatSpace()) return null

    if (state.lineStart) {
      if (stream.match("workflow")) {
        state.lineStart = false
        state.workflowName = true
        return "keyword"
      }
    }

    if (state.workflowName && stream.match(/[a-z][a-z0-9-]*/)) {
      state.workflowName = false
      return "variableName"
    }

    state.lineStart = false
    if (stream.match(/[a-z][a-z0-9-]*(?=\[)/)) return "propertyName"
    if (stream.match("[")) return "bracket"
    if (stream.match(/[^\]]+(?=\])/)) return "string"
    if (stream.match("]")) return "bracket"

    if (stream.match(/(?:skills|rank|gate):/)) {
      state.metadataValue = true
      return "keyword"
    }
    if (state.metadataValue && stream.match(/[^\n]+/)) {
      state.metadataValue = false
      return "string"
    }
    if (stream.match(/[^\n]+/)) return "comment"

    return null
  },
}

/** The workflow document syntax used by the workflow editor. */
export const workflowLanguage = StreamLanguage.define(parser)
