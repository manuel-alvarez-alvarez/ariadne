import { describe, expect, it } from "vitest"

import { workflowLanguage } from "./workflow-language"

describe("the workflow language", () => {
  it("classifies workflow names, columns, metadata, and descriptions", () => {
    const document = [
      "  workflow develop-review",
      "  develop[Develop]",
      "    Build the task.",
      "    skills: coding",
      "    rank: balanced",
      "    gate: committed",
    ].join("\n")
    const cursor = workflowLanguage.parser.parse(document).cursor()
    const tokens: string[] = []
    do {
      tokens.push(cursor.name)
    } while (cursor.next())

    expect(tokens).toEqual([
      "Document",
      "keyword",
      "variableName",
      "propertyName",
      "bracket",
      "string",
      "bracket",
      "comment",
      "keyword",
      "string",
      "keyword",
      "string",
      "keyword",
      "string",
    ])
  })
})
