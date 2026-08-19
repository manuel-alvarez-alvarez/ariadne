/**
 * One prompt of a profile, edited in place.
 *
 * Every prompt is its own save: the daemon takes them one at a time (`PUT
 * /v1/profiles/{id}/prompts/{kind}`, and the profile's own PUT for the system
 * prompt), so this is a box with its own Save rather than a field in a form
 * that posts everything at once.
 *
 * Nothing here reads the template. Placeholders are the daemon's business and a
 * briefing that drops one is explicitly allowed, so there is no validation to
 * fail and no default text on this side — {@link PromptEditorProps.onRestore}
 * asks the daemon for the default and shows what comes back.
 */

import { RotateCcwIcon } from "lucide-react"
import { type ReactNode, useEffect, useId, useState } from "react"
import { toast } from "sonner"

import { ConfirmDialog } from "@/components/confirm-dialog"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Textarea } from "@/components/ui/textarea"

interface PromptEditorProps {
  /** How the prompt is spelled on screen; also names both of its buttons. */
  label: string
  /** When the daemon sends this prompt, under the box. */
  hint: ReactNode
  /** What the daemon holds. Replaces the draft whenever it changes. */
  content: string
  /** What restoring this prompt puts back, for the confirmation. */
  restoreDescription: ReactNode
  onSave: (content: string) => Promise<unknown>
  /** Resets on the daemon and answers the restored default. */
  onRestore: () => Promise<string>
}

export function PromptEditor({
  label,
  hint,
  content,
  restoreDescription,
  onSave,
  onRestore,
}: PromptEditorProps) {
  const fieldId = useId()
  const [draft, setDraft] = useState(content)
  const [saving, setSaving] = useState(false)
  const [failure, setFailure] = useState<unknown>(null)
  const [confirming, setConfirming] = useState(false)
  const [restoring, setRestoring] = useState(false)
  const [restoreFailure, setRestoreFailure] = useState<unknown>(null)

  // The daemon is the authority on what this prompt is: whatever it answers —
  // a save, a restore, a refetch — is what the box shows next.
  useEffect(() => {
    setDraft(content)
  }, [content])

  const dirty = draft !== content

  async function save() {
    setSaving(true)
    setFailure(null)
    try {
      await onSave(draft)
      toast.success(`${label} saved`)
    } catch (error) {
      setFailure(error)
    } finally {
      setSaving(false)
    }
  }

  async function restore() {
    setRestoring(true)
    setRestoreFailure(null)
    try {
      // The restored text is taken from the answer rather than waited for
      // through the cache: an unsaved draft has to be replaced even when the
      // prompt was already at its default and nothing in the cache moved.
      const restored = await onRestore()
      setDraft(restored)
      setFailure(null)
      setConfirming(false)
      toast.success(`${label} restored to default`)
    } catch (error) {
      setRestoreFailure(error)
    } finally {
      setRestoring(false)
    }
  }

  return (
    <Field>
      <div className="flex items-center justify-between gap-2">
        <FieldLabel htmlFor={fieldId}>{label}</FieldLabel>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Restore ${label} to default`}
          onClick={() => {
            setRestoreFailure(null)
            setConfirming(true)
          }}
        >
          <RotateCcwIcon />
          Restore default
        </Button>
      </div>

      {/* `field-sizing-content` on the base textarea grows the box to its
          text, and a briefing is long: capped here, so four of them in one
          expanded row still leave the row scrollable rather than endless. */}
      <Textarea
        id={fieldId}
        spellCheck={false}
        className="max-h-96 min-h-40 resize-y font-mono text-xs leading-relaxed"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
      />

      <div className="flex flex-wrap items-center justify-between gap-2">
        <FieldDescription className="text-xs">{hint}</FieldDescription>
        <Button
          type="button"
          size="sm"
          aria-label={`Save ${label}`}
          disabled={!dirty}
          pending={saving}
          onClick={() => void save()}
        >
          Save
        </Button>
      </div>

      {failure ? <ErrorState title={`Could not save the ${lower(label)}`} error={failure} /> : null}

      <ConfirmDialog
        open={confirming}
        onClose={() => setConfirming(false)}
        title={`Restore the default ${lower(label)}?`}
        description={restoreDescription}
        confirmLabel="Restore default"
        destructive
        pending={restoring}
        error={restoreFailure}
        errorTitle={`Could not restore the ${lower(label)}`}
        onConfirm={() => void restore()}
      />
    </Field>
  )
}

/** "System prompt" mid-sentence. The kinds are all one capitalised word in. */
function lower(label: string): string {
  return label.charAt(0).toLowerCase() + label.slice(1)
}
