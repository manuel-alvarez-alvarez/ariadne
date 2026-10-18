/**
 * What Ariadne babysits any open request by hand on (032): the
 * model and effort, and the skills beside `pr-babysit`, picked by the user
 * rather than taken off the repository — a keeper session starts only when
 * asked for.
 *
 * `pr-babysit` is the keeper session's own playbook, so it is shown on and
 * cannot be turned off, and it is the whole of what a keeper starts with;
 * any skill a task agent is staffed on can join it.
 */

import { useQuery } from "@tanstack/react-query"
import { BotIcon } from "lucide-react"
import { useEffect, useState } from "react"

import type { PullRequestDto } from "@/api"
import { ErrorState } from "@/components/error-state"
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
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { skillsQueryOptions } from "@/features/skills/queries"
import { cn } from "@/lib/format"

import { useAskBabysit } from "./queries"

/** The keeper session's own playbook: always loaded, never picked. */
const PLAYBOOK = "pr-babysit"

/** What the skills start as beside the playbook: none. */
const DEFAULT_SKILLS: string[] = []

export function StartBabysitDialog({
  pull,
  open,
  onOpenChange,
  onStarted,
}: {
  pull: PullRequestDto
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Babysitting was asked for: the dialog has closed itself. */
  onStarted?: () => void
}) {
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
  const skills = useQuery({ ...skillsQueryOptions(), enabled: open })
  const ask = useAskBabysit()
  const [pin, setPin] = useState({ model: "", effort: "" })
  const [picked, setPicked] = useState<string[]>(DEFAULT_SKILLS)

  // Every open starts afresh: the last pick asked for, or nothing.
  useEffect(() => {
    if (!open) return
    setPin({ model: pull.babysit_model ?? "", effort: pull.babysit_effort ?? "" })
    setPicked(pull.babysit_skills?.length ? pull.babysit_skills : DEFAULT_SKILLS)
    ask.reset()
  }, [open, pull.babysit_model, pull.babysit_effort, pull.babysit_skills, ask.reset])

  const offered = (skills.data ?? []).filter(
    (skill) => skill.seat === "task" && skill.name !== PLAYBOOK,
  )

  function toggle(name: string) {
    setPicked((current) =>
      current.includes(name) ? current.filter((each) => each !== name) : [...current, name],
    )
  }

  function start() {
    ask.mutate(
      {
        repository_id: pull.repository_id,
        number: pull.number,
        asked: true,
        model: pin.model,
        effort: pin.effort || undefined,
        skills: picked,
      },
      {
        onSuccess: () => {
          onOpenChange(false)
          onStarted?.()
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Start Ariadne babysitting</DialogTitle>
          <DialogDescription>
            A keeper session keeps #{pull.number} open: it answers comments, fixes checks, and
            pushes its fixes onto the request's branch until the request merges or closes.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-5 py-2">
          <Field>
            <FieldLabel htmlFor="babysit-pin">Model</FieldLabel>
            <PinPicker
              id="babysit-pin"
              label="Model"
              model={pin.model}
              effort={pin.effort}
              onChange={(next) => setPin({ model: next.model, effort: next.effort })}
              models={models.data}
            />
            <FieldDescription>What the keeper runs on, and at which effort.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel>Skills</FieldLabel>
            {/* biome-ignore lint/a11y/useSemanticElements: a fieldset groups form controls of a form, and this dialog submits none */}
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Skills">
              <SkillChip name={PLAYBOOK} pressed disabled />
              {offered.map((skill) => (
                <SkillChip
                  key={skill.name}
                  name={skill.name}
                  hint={skill.summary}
                  pressed={picked.includes(skill.name)}
                  onClick={() => toggle(skill.name)}
                />
              ))}
            </div>
            <FieldDescription>
              {PLAYBOOK} is the keeper's own playbook and is always loaded.
            </FieldDescription>
          </Field>
          {ask.error ? <ErrorState title="Could not start babysitting" error={ask.error} /> : null}
        </div>
        <DialogFooter>
          <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
          <Button disabled={!pin.model} pending={ask.isPending} onClick={start}>
            <BotIcon />
            Start babysit
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function SkillChip({
  name,
  hint,
  pressed,
  disabled = false,
  onClick,
}: {
  name: string
  hint?: string
  pressed: boolean
  disabled?: boolean
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      disabled={disabled}
      title={hint}
      onClick={onClick}
      className={cn(
        "rounded-full border px-2.5 py-0.5 font-mono text-xs transition-colors",
        "focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none",
        pressed
          ? "border-primary/40 bg-primary/10 text-foreground"
          : "border-border text-muted-foreground hover:bg-muted hover:text-foreground",
        disabled && "cursor-default opacity-80",
      )}
    >
      {name}
    </button>
  )
}
