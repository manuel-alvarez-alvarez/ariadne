/**
 * The inputs of "Add machine" through SSH, and the setup request they make.
 *
 * A selection keeps only what the user changed. Every other choice comes
 * from its default: an agent starts selected when it runs on this machine,
 * GitHub CLI and every registered repository start selected, a repository
 * goes to `~/workspace/<name>`, and each sign-in happens on the new machine.
 * So a second inspection of another machine keeps the user's picks and gives
 * its new agents their own defaults.
 */

import type { RepositoryDto } from "@/api"
import type { components } from "@/api/schema"

export type Inspection = components["schemas"]["InspectionDto"]
export type SetupRequest = components["schemas"]["SetupRequest"]
type BinarySource = components["schemas"]["BinarySource"]
type SignIn = components["schemas"]["SignIn"]

/** Where one sign-in that the selection needs comes from. */
export type SignInChoice = "new" | "copy"

export interface MachineDraft {
  name: string
  ssh: string
  remote: string
  cli: string
  daemon: string
  releaseRepository: string
  releaseTag: string
  agents: Record<string, boolean>
  github: boolean | null
  repositories: Record<string, { selected?: boolean; destination?: string }>
  signIns: Record<string, SignInChoice>
}

export const EMPTY_DRAFT: MachineDraft = {
  name: "",
  ssh: "",
  remote: "",
  cli: "",
  daemon: "",
  releaseRepository: "",
  releaseTag: "",
  agents: {},
  github: null,
  repositories: {},
  signIns: {},
}

const NAME = /^[A-Za-z0-9_-]+$/

/** Why Connect cannot inspect yet, or null. */
export function connectBlocker(draft: MachineDraft): string | null {
  if (!draft.name.trim()) return "Enter a name."
  if (!NAME.test(draft.name.trim())) return "Use only letters, digits, - and _ in the name."
  if (!draft.ssh.trim()) return "Enter the SSH address."
  return null
}

/** The short name of a repository, from its forge or from its folder. */
export function repositoryName(repository: RepositoryDto): string {
  return repository.forge?.name ?? repository.path.split("/").filter(Boolean).pop() ?? repository.id
}

export function agentSelected(draft: MachineDraft, agent: Inspection["agents"][number]): boolean {
  return draft.agents[agent.id] ?? agent.on_this_machine
}

export function githubSelected(draft: MachineDraft): boolean {
  return draft.github ?? true
}

export function repositorySelected(draft: MachineDraft, repository: RepositoryDto): boolean {
  return draft.repositories[repository.id]?.selected ?? true
}

export function destinationOf(draft: MachineDraft, repository: RepositoryDto): string {
  return (
    draft.repositories[repository.id]?.destination ?? `~/workspace/${repositoryName(repository)}`
  )
}

/** The sign-ins that the selected agents and GitHub CLI need, in inspection order. */
export function neededSignIns(draft: MachineDraft, inspection: Inspection): SignIn[] {
  const needed = new Set<string>()
  for (const agent of inspection.agents)
    if (agentSelected(draft, agent) && agent.sign_in) needed.add(agent.sign_in)
  if (githubSelected(draft)) needed.add("github")
  return inspection.sign_ins.filter((signIn) => needed.has(signIn.id))
}

export function signInChoice(draft: MachineDraft, signIn: SignIn): SignInChoice {
  return signIn.copy_supported ? (draft.signIns[signIn.id] ?? "new") : "new"
}

/**
 * The programs that the Advanced inputs name, or the reason they name none
 * that setup can use. No Advanced input is no source: the daemon picks the
 * programs that inspection reports.
 */
export function sourceOf(draft: MachineDraft): { source?: BinarySource; blocker?: string } {
  const cli = draft.cli.trim()
  const daemon = draft.daemon.trim()
  const repository = draft.releaseRepository.trim()
  const tag = draft.releaseTag.trim()
  const local = cli || daemon
  const release = repository || tag
  if (local && release) return { blocker: "Use the program paths or a release, not both." }
  if (local) {
    if (!cli || !daemon) return { blocker: "Fill both program paths, or leave both empty." }
    return { source: { kind: "local", cli, daemon } }
  }
  if (release) {
    if (!repository || !tag)
      return { blocker: "Fill the release repository and the release tag, or leave both empty." }
    return { source: { kind: "release", repository, tag } }
  }
  return {}
}

/** Why Select cannot review yet, or null. */
export function selectBlocker(draft: MachineDraft, repositories: RepositoryDto[]): string | null {
  const blocker = sourceOf(draft).blocker
  if (blocker) return blocker
  for (const repository of repositories)
    if (repositorySelected(draft, repository) && !destinationOf(draft, repository).trim())
      return `Enter a destination for ${repositoryName(repository)}.`
  return null
}

/** The sign-ins that setup copies from this machine. */
export function copiedSignIns(draft: MachineDraft, inspection: Inspection): SignIn[] {
  return neededSignIns(draft, inspection).filter((signIn) => signInChoice(draft, signIn) === "copy")
}

/** The request that review and setup both take. */
export function setupRequest(
  draft: MachineDraft,
  inspection: Inspection,
  repositories: RepositoryDto[],
): SetupRequest {
  const agents = inspection.agents
    .filter((agent) => agentSelected(draft, agent))
    .map((agent) => agent.id)
  const github = githubSelected(draft)
  const credentials = copiedSignIns(draft, inspection).map((signIn) => signIn.id)
  const destinations = Object.fromEntries(
    repositories
      .filter((repository) => repositorySelected(draft, repository))
      .map((repository) => [repository.id, destinationOf(draft, repository).trim()]),
  )
  const { source } = sourceOf(draft)
  const remote = draft.remote.trim()
  return {
    ssh: draft.ssh.trim(),
    ...(remote ? { remote } : {}),
    ...(source ? { source } : {}),
    ...(agents.length || github ? { tools: { agents, github } } : {}),
    ...(credentials.length ? { credentials } : {}),
    ...(Object.keys(destinations).length ? { repositories: destinations } : {}),
  }
}

/** The Ariadne programs that inspection picked, as one line. */
export function programsLine(binaries: Inspection["binaries"]): string {
  const parts =
    binaries.method === "release"
      ? ["Release", `version ${binaries.version}`]
      : ["Built from source", `version ${binaries.version}`]
  if (binaries.method === "release" && binaries.tag) parts.push(`tag ${binaries.tag}`)
  if (binaries.commit) parts.push(`commit ${binaries.commit.slice(0, 8)}`)
  if (binaries.uncommitted) parts.push("uncommitted changes")
  return parts.join(" · ")
}

/** The display name of an agent that only its id names: `codex-acp` is Codex. */
export function agentName(id: string): string {
  return id
    .replace(/-acp$/, "")
    .split("-")
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ")
}
