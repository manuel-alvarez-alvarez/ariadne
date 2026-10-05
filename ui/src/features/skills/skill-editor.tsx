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
 *
 * The draft is never overwritten out from under a typing user. `synced` holds
 * the document the box was last set from; the row is followed onto a new
 * document only while the box still reads exactly that — a clean draft. A
 * dirty one holds its ground, and `changedElsewhere` says there is a newer
 * version waiting, through `screen.getByText` in the tests rather than
 * silently losing what was typed. `saving` is the one exception: the document
 * a Save just sent is expected back on the row, and its return is not a
 * change from anywhere else.
 *
 * The screen this is opened in owns leaving it: `onDirtyChange` reports every
 * flip of the draft's dirtiness, so a selection change, a Back or a route away
 * can be asked about before the draft is gone.
 */

import { RefreshCwIcon, Trash2Icon, Undo2Icon } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { SkillDto } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"
import { cn, describeError } from "@/lib/format"

import { useDeleteSkill, useResetSkill, useUpdateSkill } from "./queries"

export function SkillEditor({
  skill,
  onDeleted,
  onDirtyChange,
}: {
  skill: SkillDto
  onDeleted: () => void
  /** Told on every change of whether the draft holds edits the row does not. */
  onDirtyChange?: (dirty: boolean) => void
}) {
  const [document, setDocument] = useState(skill.document)
  const [confirmReset, setConfirmReset] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [changedElsewhere, setChangedElsewhere] = useState(false)

  const update = useUpdateSkill()
  const reset = useResetSkill()
  const remove = useDeleteSkill()

  const name = useRef(skill.name)
  // What the box was last set from: the baseline a clean draft still equals,
  // and a dirty one no longer does.
  const synced = useRef(skill.document)
  // The document a Save sent, until the row carries it back. Its own return
  // is read quietly rather than as a change from elsewhere.
  const saving = useRef<string | null>(null)
  // The current box content, without putting it in the effect below: that
  // would run it on every keystroke rather than only when the row changes.
  const draft = useRef(document)
  draft.current = document

  useEffect(() => {
    if (skill.name !== name.current) {
      name.current = skill.name
      synced.current = skill.document
      saving.current = null
      setDocument(skill.document)
      setChangedElsewhere(false)
      return
    }
    if (skill.document === synced.current) return
    if (saving.current !== null && skill.document === saving.current) {
      synced.current = skill.document
      saving.current = null
      return
    }
    if (draft.current === synced.current) {
      synced.current = skill.document
      setDocument(skill.document)
    } else {
      setChangedElsewhere(true)
    }
  }, [skill.name, skill.document])

  const dirty = document !== skill.document

  useEffect(() => {
    onDirtyChange?.(dirty)
    // A skill that goes away under this editor — deleted, or simply no
    // longer selected — takes the question of leaving it with it.
    return () => onDirtyChange?.(false)
  }, [dirty, onDirtyChange])

  /** Throws the draft away and reads the row's current document instead. */
  function syncToRow() {
    synced.current = skill.document
    setDocument(skill.document)
    setChangedElsewhere(false)
  }

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

      {changedElsewhere ? (
        <Alert>
          <RefreshCwIcon />
          <AlertTitle>This skill changed elsewhere</AlertTitle>
          <AlertDescription>
            Your edits are kept here. Load the new version to see what changed underneath them.
          </AlertDescription>
          <AlertAction>
            <Button variant="outline" size="sm" onClick={syncToRow}>
              Load new version
            </Button>
          </AlertAction>
        </Alert>
      ) : null}

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
          onClick={() => {
            saving.current = document
            update.mutate(
              { name: skill.name, body: { document } },
              {
                onError: () => {
                  saving.current = null
                },
              },
            )
          }}
        >
          {update.isPending ? "Saving…" : "Save"}
        </Button>
        <Button variant="ghost" disabled={!dirty} onClick={syncToRow}>
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
        onConfirm={() =>
          remove.mutate(skill.name, {
            onSuccess: () => {
              // The skill is gone, so there is nothing left to ask about
              // leaving it: report clean before the delete navigates away.
              onDirtyChange?.(false)
              onDeleted()
            },
          })
        }
      />
    </div>
  )
}
