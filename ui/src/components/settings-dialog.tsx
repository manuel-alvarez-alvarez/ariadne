/**
 * Where the daemon lives. This is the only thing the UI needs configured: the
 * address of `tcp_listen` in `~/.ariadne/config.toml`.
 */

import { useEffect, useState } from "react"
import { toast } from "sonner"

import { DEFAULT_BASE_URL, normalizeBaseUrl } from "@/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { useSettingsStore } from "@/stores/settings"

export function SettingsDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const baseUrl = useSettingsStore((state) => state.baseUrl)
  const setBaseUrl = useSettingsStore((state) => state.setBaseUrl)

  const [draft, setDraft] = useState(baseUrl)
  const [error, setError] = useState<string | null>(null)

  // Re-open always shows what is actually configured, not a stale draft.
  useEffect(() => {
    if (open) {
      setDraft(baseUrl)
      setError(null)
    }
  }, [open, baseUrl])

  function submit(event: React.FormEvent) {
    event.preventDefault()
    const problem = validateBaseUrl(draft)
    if (problem) {
      setError(problem)
      return
    }
    const normalized = normalizeBaseUrl(draft)
    setBaseUrl(normalized)
    toast.success("Daemon URL updated", { description: normalized })
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>Settings</DialogTitle>
            <DialogDescription>
              Ariadne talks to the daemon over HTTP only, so it needs the address the daemon listens
              on.
            </DialogDescription>
          </DialogHeader>
          <div className="py-4">
            <Field data-invalid={error ? "" : undefined}>
              <FieldLabel htmlFor="daemon-base-url">Daemon URL</FieldLabel>
              <Input
                id="daemon-base-url"
                value={draft}
                onChange={(event) => {
                  setDraft(event.target.value)
                  setError(null)
                }}
                placeholder={DEFAULT_BASE_URL}
                spellCheck={false}
                autoComplete="off"
                aria-invalid={error ? true : undefined}
              />
              {error ? (
                <FieldError>{error}</FieldError>
              ) : (
                <FieldDescription>
                  The <code className="font-mono">tcp_listen</code> address from{" "}
                  <code className="font-mono">~/.ariadne/config.toml</code>. Changing it clears
                  everything cached from the previous daemon.
                </FieldDescription>
              )}
            </Field>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => {
                setDraft(DEFAULT_BASE_URL)
                setError(null)
              }}
            >
              Reset to default
            </Button>
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            {/* No `pending`: this writes to the local store and nothing else.
                There is no request to wait for, so a spinner would be a lie. */}
            <Button type="submit">Save</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Accept only absolute http(s) URLs — that is all `fetch`/`EventSource` take. */
function validateBaseUrl(value: string): string | null {
  const trimmed = value.trim()
  if (trimmed.length === 0) return "Enter the daemon URL."
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return "Not a valid URL, e.g. http://127.0.0.1:7676"
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return "Only http:// and https:// URLs are supported."
  }
  return null
}
