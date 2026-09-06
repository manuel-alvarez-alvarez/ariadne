/**
 * The skills screen: every skill on the left, the selected one's document on
 * the right.
 *
 * A skill is a document, so the screen is a list beside an editor rather than
 * a page per skill — the same shape the profiles screen had, and for the same
 * reason: what you come here to do is read one text and change it, with the
 * others in reach.
 *
 * They are grouped by where the document came from, because that is what says
 * what can be done to it: the ones Ariadne ships are reset, and the ones you
 * wrote are deleted.
 *
 * Below `md` there is no room for both columns: the list is the screen, and a
 * selection replaces it until it is cleared.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { SkillDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/format"
import { SKILL_PARAM } from "@/routes/paths"

import { CreateSkillDialog } from "./create-skill-dialog"
import { skillsQueryOptions } from "./queries"
import { SkillEditor } from "./skill-editor"

export function SkillsPage() {
  const [createOpen, setCreateOpen] = useState(false)
  const [search, setSearch] = useSearchParams()
  const selectedName = search.get(SKILL_PARAM)

  const skills = useQuery(skillsQueryOptions())
  const selected = skills.data?.find((skill) => skill.name === selectedName)

  /**
   * Selects a skill. It pushes: a selection is what a link points at, so Back
   * has to return to the one before it and Forward to bring it back. Every
   * other param is kept, so a panel open over the screen stays open.
   */
  function select(name: string) {
    const next = new URLSearchParams(search)
    next.set(SKILL_PARAM, name)
    setSearch(next)
  }

  /**
   * Clears the selection in place. This is the way back to the list where the
   * list is not beside the editor, and where the selected skill is gone; in
   * neither case is the user going somewhere, so nothing is pushed.
   */
  function clearSelection() {
    const next = new URLSearchParams(search)
    next.delete(SKILL_PARAM)
    setSearch(next, { replace: true })
  }

  return (
    // A fixed-height column against the shell's `<main>`, the way the board
    // is: the two columns below scroll on their own, so the screen must not
    // grow with them.
    <div className="flex h-full min-h-0 flex-col gap-4">
      <PageHeader
        title="Skills"
        description="What an agent knows. Each skill is one document about one kind of work, and a task gives each of its agents the ones that work needs."
        actions={
          <Button onClick={() => setCreateOpen(true)}>
            <PlusIcon />
            New skill
          </Button>
        }
      />

      {skills.isPending ? (
        <LoadingSkills />
      ) : skills.isError ? (
        <ErrorState
          title="Could not load skills"
          error={skills.error}
          onRetry={() => void skills.refetch()}
          showIcon
        />
      ) : (
        <div className="flex min-h-0 flex-1 gap-6">
          <SkillList
            skills={skills.data}
            selectedName={selectedName}
            search={search}
            className={cn(selectedName ? "hidden md:flex" : "flex")}
          />
          <section
            aria-label="Selected skill"
            className={cn(
              "min-h-0 min-w-0 flex-1 flex-col",
              selectedName ? "flex" : "hidden md:flex",
            )}
          >
            {selected ? (
              // Keyed by the skill: a selection is a fresh editor, never one
              // skill's document left in the box under another's name.
              <SkillEditor key={selected.name} skill={selected} onDeleted={clearSelection} />
            ) : selectedName ? (
              // A link to a skill that is gone — deleted since, or never this
              // daemon's — lands here rather than on nothing at all.
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="No skill by that name."
                description="It may have been deleted since the link was made."
                action={
                  <Button variant="outline" size="sm" onClick={clearSelection}>
                    Back to the list
                  </Button>
                }
              />
            ) : (
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="Select a skill, or write one."
              />
            )}
          </section>
        </div>
      )}

      <CreateSkillDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={(skill) => select(skill.name)}
      />
    </div>
  )
}

/**
 * The list: the shipped skills and yours under their own headings, the
 * selected one marked, and a box above them to narrow by name or by what the
 * skill says it is for.
 */
function SkillList({
  skills,
  selectedName,
  search,
  className,
}: {
  skills: SkillDto[]
  selectedName: string | null
  /** The screen's params, kept on every selection link. */
  search: URLSearchParams
  className?: string
}) {
  const [filter, setFilter] = useState("")
  const needle = filter.trim().toLowerCase()
  const shown = needle
    ? skills.filter(
        (skill) =>
          skill.name.toLowerCase().includes(needle) || skill.summary.toLowerCase().includes(needle),
      )
    : skills

  const groups = [
    { key: "shipped", title: "Shipped with Ariadne", skills: shown.filter((s) => s.builtin) },
    { key: "yours", title: "Yours", skills: shown.filter((s) => !s.builtin) },
  ].filter((group) => group.skills.length > 0)

  function href(name: string): string {
    const next = new URLSearchParams(search)
    next.set(SKILL_PARAM, name)
    return `?${next.toString()}`
  }

  return (
    <nav aria-label="Skills" className={cn("w-full min-w-0 flex-col gap-3 md:w-72", className)}>
      <Input
        value={filter}
        placeholder="Filter skills"
        aria-label="Filter skills"
        autoComplete="off"
        onChange={(event) => setFilter(event.target.value)}
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups.length === 0 ? (
          <p className="px-1 py-6 text-muted-foreground text-sm">No skill matches “{filter}”.</p>
        ) : (
          groups.map((group) => (
            // Named by its own heading, so the group a row belongs to — and
            // so what can be done to that row — reaches a screen reader too.
            <section key={group.key} aria-labelledby={`skills-${group.key}`} className="mb-4">
              <h3
                id={`skills-${group.key}`}
                className="px-1 pb-1 font-medium text-muted-foreground text-xs uppercase tracking-wide"
              >
                {group.title}
              </h3>
              <ul className="flex flex-col gap-0.5">
                {group.skills.map((skill) => (
                  <li key={skill.name}>
                    <Link
                      to={href(skill.name)}
                      aria-current={skill.name === selectedName ? "true" : undefined}
                      className={cn(
                        "flex flex-col gap-0.5 rounded-md px-2 py-1.5 text-sm hover:bg-accent",
                        skill.name === selectedName && "bg-accent",
                      )}
                    >
                      <span className="flex items-baseline gap-2">
                        <span className="min-w-0 truncate font-medium">{skill.name}</span>
                        {/* Only worth a badge where it is not the default: a
                            shipped skill somebody has rewritten is the one
                            thing in this list that behaves unexpectedly. */}
                        {skill.builtin && !skill.document_is_default ? (
                          <Badge variant="outline" className="shrink-0">
                            edited
                          </Badge>
                        ) : null}
                      </span>
                      <span className="truncate text-muted-foreground text-xs">
                        {skill.summary}
                      </span>
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          ))
        )}
      </div>
    </nav>
  )
}

/** Both columns, before the list has arrived. */
function LoadingSkills() {
  return (
    <div className="flex min-h-0 flex-1 gap-6">
      <div className="flex w-full flex-col gap-2 md:w-72">
        {["a", "b", "c", "d", "e", "f", "g", "h"].map((row) => (
          <Skeleton key={row} className="h-10 w-full" />
        ))}
      </div>
      <Skeleton className="hidden min-h-0 flex-1 md:block" />
    </div>
  )
}
