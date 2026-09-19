/**
 * The tabs of the knowledge screen (022), in the order they are shown.
 *
 * One entry per tab, and nothing else to change to add one: its id is what
 * `?tab=` holds, and `render` draws it from what the pickers above picked.
 */

import type { ReactNode } from "react"

import type { RepositoryDto } from "@/api"
import { EmptyState } from "@/components/empty-state"

import { OverviewTab } from "./overview-tab"
import { RepositoriesTab } from "./repositories-tab"

/** What the pickers above the tabs picked, and the way to pick again. */
interface KnowledgeTabContext {
  repositories: RepositoryDto[]
  repository: RepositoryDto
  gitRef: string
  pickRepository: (repositoryId: string) => void
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
  { id: "symbols", label: "Symbols", render: () => <Coming /> },
  { id: "impact", label: "Impact & path", render: () => <Coming /> },
  { id: "files", label: "Files", render: () => <Coming /> },
]

function Coming() {
  return <EmptyState emphasis="quiet" className="py-12" title="This tab is coming." />
}
