import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"

import { skillsQueryOptions } from "@/features/skills/queries"
import { describeError } from "@/lib/format"

import { useWorkflowParse, type WorkflowParseResult } from "./use-workflow-parse"
import { workflowAutocomplete, workflowHover } from "./workflow-help-extensions"

type WorkflowEditing = {
  parsed: WorkflowParseResult
  error: { line: number; message: string } | null
  extensions: ReturnType<typeof workflowAutocomplete>[]
}

/** The parse, its error mark and the skill-aware extensions a workflow document editor needs. */
export function useWorkflowEditing(document: string): WorkflowEditing {
  const parsed = useWorkflowParse(document)
  const error = parsed.errorLine
    ? { line: parsed.errorLine, message: describeError(parsed.error) }
    : null
  const skills = useQuery(skillsQueryOptions()).data ?? []
  const extensions = useMemo(() => [workflowAutocomplete(skills), workflowHover(skills)], [skills])
  return { parsed, error, extensions }
}
