import type { UseQueryResult } from "@tanstack/react-query"
import { useQuery } from "@tanstack/react-query"
import { useEffect, useState } from "react"

import { ApiError, api, type ParsedWorkflowDto, qk, unwrap } from "@/api"

export type WorkflowParseResult = UseQueryResult<ParsedWorkflowDto, Error> & {
  errorLine: number | null
}

function errorLine(error: unknown): number | null {
  if (!ApiError.is(error) || typeof error.details !== "object" || error.details === null)
    return null
  const line = (error.details as { line?: unknown }).line
  return typeof line === "number" ? line : null
}

/** Parse a settled workflow draft once for both its editor and preview. */
export function useWorkflowParse(document: string): WorkflowParseResult {
  const [settledDocument, setSettledDocument] = useState(document)

  useEffect(() => {
    const timeout = window.setTimeout(() => setSettledDocument(document), 250)
    return () => window.clearTimeout(timeout)
  }, [document])

  const parsed = useQuery({
    queryKey: qk.workflows.parse(settledDocument),
    queryFn: () =>
      unwrap(api().POST("/v1/workflows/parse", { body: { document: settledDocument } })),
  })
  return { ...parsed, errorLine: parsed.isError ? errorLine(parsed.error) : null }
}
