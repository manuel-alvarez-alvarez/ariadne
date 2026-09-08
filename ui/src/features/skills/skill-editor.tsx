/**
 * One skill's document, editable.
 *
 * A skill owns exactly one text — the whole `SKILL.md`, frontmatter and body —
 * so this is a single textarea and the two things that can be done to it. What
 * differs is where the text came from: a skill Ariadne ships is *reset* back
 * to the shipped one and cannot be deleted, and a skill of the user's own is
 * *deleted* and cannot be reset, because there is nothing behind it to go back
 * to. The daemon refuses the wrong one either way; the screen only offers the
 * right one.
 */

import { Trash2Icon, Undo2Icon } from "lucide-react"
import { useEffect, useState } from "react"

import type { SkillDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { cn, describeError } from "@/lib/format"

import { useDeleteSkill, useResetSkill, useUpdateSkill } from "./queries"

export function SkillEditor({ skill, onDeleted }: { skill: SkillDto; onDeleted: () => void }) {
  const [document, setDocument] = useState(skill.document)
  const [confirmReset, setConfirmReset] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const update = useUpdateSkill()
  const reset = useResetSkill()
  const remove = useDeleteSkill()

  // The document follows the row: a reset, or an edit that arrived on the
  // event stream, replaces what is in the box rather than leaving the screen
  // showing a text the daemon no longer holds.
  useEffect(() => setDocument(skill.document), [skill.document])

  const dirty = document !== skill.document
  const error = update.error

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      <header className="flex flex-wrap items-baseline gap-2">
        <h2 className="font-medium text-lg">{skill.name}</h2>
        <Badge variant={skill.builtin ? "secondary" : "outline"}>
          {skill.builtin ? (skill.document_is_default ? "shipped" : "shipped · edited") : "yours"}
        </Badge>
        {/* The orchestrator's own playbook stays here to inspect, edit and
            reset, but it is not a task staffing choice — this says so. */}
        {skill.seat === "orchestrator" ? <Badge variant="outline">orchestrator only</Badge> : null}
        <p className="w-full text-muted-foreground text-sm">{skill.summary}</p>
      </header>

      <Field className="flex min-h-0 flex-1 flex-col">
        <FieldLabel htmlFor="skill-document">Document</FieldLabel>
        <FieldDescription>
          The whole <code>SKILL.md</code>: YAML frontmatter naming the skill and describing it in
          one line, then the body. The description is what an agent reads before it opens the
          document.
        </FieldDescription>
        <Textarea
          id="skill-document"
          value={document}
          spellCheck={false}
          onChange={(event) => setDocument(event.target.value)}
          className="min-h-0 flex-1 resize-none font-mono text-xs"
        />
      </Field>

      {error ? (
        <p role="alert" className="text-destructive text-sm">
          {describeError(error)}
        </p>
      ) : null}

      <footer className="flex flex-wrap items-center gap-2">
        <Button
          disabled={!dirty || update.isPending}
          onClick={() => update.mutate({ name: skill.name, body: { document } })}
        >
          {update.isPending ? "Saving…" : "Save"}
        </Button>
        <Button variant="ghost" disabled={!dirty} onClick={() => setDocument(skill.document)}>
          Discard
        </Button>

        <span className="flex-1" />

        {skill.builtin ? (
          <Button
            variant="outline"
            // Nothing to go back to while it is already on the shipped text.
            disabled={skill.document_is_default || reset.isPending}
            onClick={() => setConfirmReset(true)}
          >
            <Undo2Icon />
            Reset
          </Button>
        ) : (
          <Button
            variant="outline"
            className={cn("text-destructive")}
            onClick={() => setConfirmDelete(true)}
          >
            <Trash2Icon />
            Delete
          </Button>
        )}
      </footer>

      <ConfirmDialog
        open={confirmReset}
        onClose={() => setConfirmReset(false)}
        title={`Reset ${skill.name}?`}
        description="Throws away the text written over this skill and goes back to the one Ariadne ships. Agents staffed on it read the shipped text from their next launch."
        confirmLabel="Reset"
        pending={reset.isPending}
        error={reset.error}
        onConfirm={() => reset.mutate(skill.name, { onSuccess: () => setConfirmReset(false) })}
      />
      <ConfirmDialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={`Delete ${skill.name}?`}
        description="This skill is yours, so there is no shipped text behind it: deleting it is permanent. It is refused while any staffed agent still loads it."
        confirmLabel="Delete"
        destructive
        pending={remove.isPending}
        error={remove.error}
        onConfirm={() => remove.mutate(skill.name, { onSuccess: onDeleted })}
      />
    </div>
  )
}
