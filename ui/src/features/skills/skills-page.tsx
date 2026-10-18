/**
 * The skills screen: a combobox over every skill, the selected one's document
 * below it.
 *
 * A skill is a document, so the screen is a picker beside an editor rather
 * than a page per skill. The catalog is searched rather than scrolled — a
 * filter box narrows it by name or by what the skill says it is for — and the
 * two groups say what can be done to a row: the ones Ariadne ships are reset,
 * and the ones written here are deleted.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useState } from "react"
import { useBlocker, useSearchParams } from "react-router-dom"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { EmptyState } from "@/components/empty-state"
import { EntityCombobox } from "@/components/entity-combobox"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
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

  // The editor reports its own dirtiness; what happens with that is the
  // screen's call, since leaving — to another skill or off the screen
  // entirely — is a navigation the screen is the one to see coming.
  const [dirty, setDirty] = useState(false)
  const blocker = useBlocker(dirty)

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
   * Clears the selection in place. This is the way back to nothing selected,
   * where the selected skill is gone; the user is not going anywhere, so
   * nothing is pushed.
   */
  function clearSelection() {
    const next = new URLSearchParams(search)
    next.delete(SKILL_PARAM)
    setSearch(next, { replace: true })
  }

  return (
    // A fixed-height column against the shell's `<main>`, the way the board
    // is: the editor below scrolls on its own, so the screen must not grow
    // with it.
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
        <div className="flex min-h-0 flex-1 flex-col gap-4">
          <EntityCombobox
            id="skill-combobox"
            label="Skill"
            entityLabel="skill"
            placeholder="Select a skill…"
            selectedName={selectedName}
            onSelect={select}
            entities={skills.data.map((skill) => ({
              name: skill.name,
              details: skill.summary,
              builtin: skill.builtin,
              // Only worth a badge where it is not the default: a shipped
              // skill somebody has rewritten is the one thing here that
              // behaves unexpectedly.
              badge:
                skill.builtin && !skill.document_is_default ? (
                  <Badge variant="outline" className="shrink-0">
                    edited
                  </Badge>
                ) : undefined,
            }))}
          />
          <section aria-label="Selected skill" className="flex min-h-0 min-w-0 flex-1 flex-col">
            {selected ? (
              // Not keyed by the skill: a selection staying the same instance
              // is what lets a dirty draft survive a switch that gets asked
              // about and then cancelled, rather than one that always wipes
              // the box clean on the spot.
              <SkillEditor skill={selected} onDeleted={clearSelection} onDirtyChange={setDirty} />
            ) : selectedName ? (
              // A pick of a skill that is gone — deleted since, or never this
              // daemon's — lands here rather than on nothing at all.
              <EmptyState
                emphasis="quiet"
                className="my-auto"
                title="No skill by that name."
                description="It may have been deleted since the link was made."
                action={
                  <Button variant="outline" size="sm" onClick={clearSelection}>
                    Clear selection
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

      <ConfirmDialog
        open={blocker.state === "blocked"}
        onClose={() => {
          if (blocker.state === "blocked") blocker.reset()
        }}
        title="Discard changes?"
        description="This skill has unsaved changes. Leaving now drops them."
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => {
          if (blocker.state === "blocked") blocker.proceed()
        }}
      />
    </div>
  )
}

/** Both the combobox and the editor, before the catalog has arrived. */
function LoadingSkills() {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <Skeleton className="h-8 w-full sm:w-96" />
      <Skeleton className="min-h-0 flex-1" />
    </div>
  )
}
