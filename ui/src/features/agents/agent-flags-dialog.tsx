/**
 * Edit dialog for one agent kind's extra flags.
 *
 * There is nothing to create and nothing to delete — the three agent kinds are
 * the daemon's, and every one of them always has a flag list — so this is the
 * edit half of the profiles dialog and no more: repeatable rows with add and
 * remove, and a submit that replaces the list whole.
 *
 * "Restore defaults" fills the rows with what Ariadne ships for the kind and
 * writes nothing on its own, the same way the prompt editors' own restore
 * does: the daemon hands the defaults out with the config, and sending them
 * back is all a reset is.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { PlusIcon, RotateCcwIcon, XIcon } from "lucide-react"
import { useEffect } from "react"
import { useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"

import type { AgentConfigDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
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
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { AGENT_KIND_LABELS, describeError } from "@/lib/format"
import {
  type AgentFlagsFormValues,
  agentFlagsSchema,
  cleanFlags,
  flagRows,
  sameFlags,
} from "./agent-flags-values"
import { useUpdateAgentConfig } from "./queries"

const EMPTY_VALUES: AgentFlagsFormValues = { flags: [] }

export function AgentFlagsDialog({
  open,
  onOpenChange,
  config,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The agent being edited, or null while the dialog has never been opened. */
  config: AgentConfigDto | null
}) {
  const updateConfig = useUpdateAgentConfig()

  const form = useForm<AgentFlagsFormValues>({
    resolver: zodResolver(agentFlagsSchema),
    defaultValues: EMPTY_VALUES,
  })
  const { control, formState, handleSubmit, register, reset, setError, watch } = form
  const flags = useFieldArray({ control, name: "flags" })

  // Every open starts from what is stored, never from the previous attempt.
  // Keyed off the dialog opening rather than the prop: the write patches the
  // list this config comes from, and re-seeding on that would undo edits.
  useEffect(() => {
    if (!open) return
    reset(config ? { flags: flagRows(config.extra_flags) } : EMPTY_VALUES)
  }, [open, config, reset])

  const rows = watch("flags")
  const defaults = config?.default_flags ?? []
  const atDefaults = sameFlags(cleanFlags(rows), defaults)

  async function submit(values: AgentFlagsFormValues) {
    if (!config) return
    const extraFlags = cleanFlags(values.flags)
    try {
      await updateConfig.mutateAsync({ kind: config.agent_kind, extraFlags })
      toast.success("Flags saved", {
        description:
          extraFlags.length > 0
            ? `${AGENT_KIND_LABELS[config.agent_kind]} is launched with ${extraFlags.join(" ")}.`
            : `${AGENT_KIND_LABELS[config.agent_kind]} is launched with no extra flags.`,
      })
      onOpenChange(false)
    } catch (error) {
      setError("root", { message: describeError(error) })
    }
  }

  const label = config ? AGENT_KIND_LABELS[config.agent_kind] : ""

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={handleSubmit(submit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{label} flags</DialogTitle>
            <DialogDescription>
              Appended to {label}'s argv on every spawn and resume, after the arguments Ariadne
              needs itself. An edit lands on the next launch; sessions already running keep the
              flags they were started with.
            </DialogDescription>
          </DialogHeader>

          <FieldGroup>
            <Field>
              <FieldLabel>Extra flags</FieldLabel>
              {flags.fields.length > 0 ? (
                <div className="flex flex-col gap-2">
                  {flags.fields.map((flag, index) => (
                    <div key={flag.id} className="flex items-center gap-2">
                      <Input
                        aria-label={`Flag ${index + 1}`}
                        placeholder="--dangerously-skip-permissions"
                        autoComplete="off"
                        spellCheck={false}
                        className="font-mono"
                        {...register(`flags.${index}.value`)}
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label={`Remove flag ${index + 1}`}
                        onClick={() => flags.remove(index)}
                      >
                        <XIcon />
                      </Button>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  None — {label} is launched with Ariadne's own arguments only.
                </p>
              )}
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => flags.append({ value: "" })}
                >
                  <PlusIcon />
                  Add flag
                </Button>
                {atDefaults ? null : (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => flags.replace(flagRows(defaults))}
                  >
                    <RotateCcwIcon />
                    Restore defaults
                  </Button>
                )}
              </div>
              <FieldDescription>
                One flag per row, as it would be typed on the command line. Blank rows are dropped,
                and an empty list is a legitimate answer.
              </FieldDescription>
            </Field>
          </FieldGroup>

          {formState.errors.root ? (
            <Alert variant="destructive">
              <AlertTitle>Could not save the flags</AlertTitle>
              <AlertDescription>{formState.errors.root.message}</AlertDescription>
            </Alert>
          ) : null}

          <DialogFooter>
            <DialogClose
              render={<Button type="button" variant="outline" disabled={updateConfig.isPending} />}
            >
              Cancel
            </DialogClose>
            <Button type="submit" pending={updateConfig.isPending}>
              Save flags
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
