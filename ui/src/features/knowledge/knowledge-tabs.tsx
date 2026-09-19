/**
 * The tabs of the knowledge screen (022), in the order they are shown.
 *
 * One entry per tab, and nothing else to change to add one: its id is what
 * `?tab=` holds, and `render` draws it from what the pickers above picked.
 */

import type { ReactNode } from "react"

import type { RepositoryDto } from "@/api"

import { FilesTab } from "./files-tab"
import { ImpactTab } from "./impact-tab"
import { OverviewTab } from "./overview-tab"
import { RepositoriesTab } from "./repositories-tab"
import { SymbolsTab } from "./symbols-tab"

/** What the pickers above the tabs picked, and the way to pick again. */
interface KnowledgeTabContext {
  repositories: RepositoryDto[]
  repository: RepositoryDto
  gitRef: string
  symbolName: string
  pickRepository: (repositoryId: string) => void
  pickSymbol: (name: string, repositoryId: string, gitRef: string) => void
}

interface KnowledgeTab {
  id: string
  label: string
  render: (context: KnowledgeTabContext) => ReactNode
}

export const KNOWLEDGE_TABS: readonly KnowledgeTab[] = [
  {
    id: "overview",
    label: "Overview",
    render: ({ repositories }) => <OverviewTab repositories={repositories} />,
  },
  {
    id: "repositories",
    label: "Repositories",
    render: ({ repositories, repository, gitRef, pickRepository }) => (
      <RepositoriesTab
        repositories={repositories}
        repositoryId={repository.id}
        gitRef={gitRef}
        onPickRepository={pickRepository}
      />
    ),
  },
  {
    id: "symbols",
    label: "Symbols",
    render: ({ repositories, repository, gitRef, symbolName, pickSymbol }) => (
      <SymbolsTab
        repositories={repositories}
        repositoryId={repository.id}
        gitRef={gitRef}
        symbolName={symbolName}
        onNavigateSymbol={pickSymbol}
      />
    ),
  },
  {
    id: "impact",
    label: "Impact & path",
    render: ({ repository, gitRef }) => <ImpactTab repositoryId={repository.id} gitRef={gitRef} />,
  },
  {
    id: "files",
    label: "Files",
    render: ({ repository, gitRef }) => <FilesTab repositoryId={repository.id} gitRef={gitRef} />,
  },
]
