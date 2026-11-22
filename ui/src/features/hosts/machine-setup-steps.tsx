/**
 * The steps of "Add machine" through SSH that show what inspection found:
 * the step bar, Select and Review. `add-machine-dialog.tsx` holds their
 * state and calls the daemon.
 */

import { CheckIcon } from "lucide-react"
import { type ReactNode, useId } from "react"

import type { RepositoryDto } from "@/api"
import type { components } from "@/api/schema"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { cn } from "@/lib/format"
import {
  agentSelected,
  copiedSignIns,
  destinationOf,
  githubSelected,
  type Inspection,
  type MachineDraft,
  neededSignIns,
  programsLine,
  repositoryName,
  repositorySelected,
  type SignInChoice,
  signInChoice,
} from "./machine-setup"
import { CommandLine } from "./setup-progress"

type SetupGroup = components["schemas"]["SetupGroup"]
type ManualAction = components["schemas"]["ManualAction"]
type SetupReview = components["schemas"]["SetupReviewDto"]

const INSTALL_STEPS = ["Connect", "Select", "Review", "Install"] as const
export type InstallStep = (typeof INSTALL_STEPS)[number]

/** The four steps of an installation, and where the user is in them. */
export function StepBar({ current }: { current: InstallStep }) {
  const position = INSTALL_STEPS.indexOf(current)
  return (
    <ol aria-label="Steps" className="flex items-center gap-2 text-sm">
      {INSTALL_STEPS.map((step, index) => (
        <li
          key={step}
          aria-current={index === position ? "step" : undefined}
          className={cn(
            "flex flex-1 items-center gap-2",
            index === position ? "font-medium text-foreground" : "text-muted-foreground",
          )}
        >
          <span
            aria-hidden
            className={cn(
              "flex size-5 shrink-0 items-center justify-center rounded-full border text-xs",
              index < position && "border-primary bg-primary text-primary-foreground",
              index === position && "border-primary text-primary",
            )}
          >
            {index < position ? <CheckIcon className="size-3" /> : index + 1}
          </span>
          <span>{step}</span>
          {index < INSTALL_STEPS.length - 1 ? (
            <span aria-hidden className="h-px flex-1 bg-border" />
          ) : null}
        </li>
      ))}
    </ol>
  )
}

/** A titled group of the form, for the controls that belong together. */
export function Section({
  title,
  description,
  children,
}: {
  title: string
  description?: ReactNode
  children: ReactNode
}) {
  return (
    <fieldset className="flex min-w-0 flex-col gap-3">
      <legend className="mb-1 text-sm font-medium">{title}</legend>
      {description ? <FieldDescription className="-mt-2">{description}</FieldDescription> : null}
      {children}
    </fieldset>
  )
}

/** Who installs one item, in the words of the Select step. */
function GroupLine({ group, manual }: { group: SetupGroup; manual?: ManualAction | null }) {
  if (group === "ariadne_installs") return <span>Ariadne installs it.</span>
  if (group === "already_present") return <span>Already on the new machine.</span>
  return (
    <span className="flex flex-col gap-1">
      <span>{manual?.command ? "You install it by hand:" : "You install it by hand."}</span>
      {manual?.command ? <CommandLine command={manual.command} /> : null}
      {manual && !manual.command ? <span>{manual.reason}</span> : null}
    </span>
  )
}

/** One checkbox with its label and the line under it. */
export function CheckLine({
  label,
  checked,
  onChange,
  children,
}: {
  label: string
  checked: boolean
  onChange: (checked: boolean) => void
  children?: ReactNode
}) {
  const id = useId()
  return (
    <div className="flex items-start gap-3">
      <Checkbox
        id={id}
        aria-labelledby={`${id}-label`}
        aria-describedby={children ? `${id}-line` : undefined}
        checked={checked}
        onCheckedChange={(next) => onChange(next === true)}
        className="mt-0.5"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <FieldLabel id={`${id}-label`} htmlFor={id} className="font-medium">
          {label}
        </FieldLabel>
        {children ? (
          <div id={`${id}-line`} className="text-sm text-muted-foreground">
            {children}
          </div>
        ) : null}
      </div>
    </div>
  )
}

export function SelectStep({
  draft,
  inspection,
  repositories,
  repositoriesError,
  change,
}: {
  draft: MachineDraft
  inspection: Inspection
  repositories: RepositoryDto[]
  repositoriesError: ReactNode
  change: (patch: (draft: MachineDraft) => MachineDraft) => void
}) {
  const signIns = neededSignIns(draft, inspection)
  return (
    <>
      <Section title="Ariadne programs">
        <p className="font-mono text-sm">{programsLine(inspection.binaries)}</p>
        <FieldDescription>
          Inspection picked these programs to match this machine. Change them under Advanced.
        </FieldDescription>
      </Section>

      <Section
        title="Agents and tools"
        description="The agents that run on this machine start selected."
      >
        {inspection.agents.map((agent) => (
          <CheckLine
            key={agent.id}
            label={agent.name}
            checked={agentSelected(draft, agent)}
            onChange={(checked) =>
              change((d) => ({ ...d, agents: { ...d.agents, [agent.id]: checked } }))
            }
          >
            <GroupLine group={agent.group} manual={agent.manual} />
          </CheckLine>
        ))}
        <CheckLine
          label="GitHub CLI"
          checked={githubSelected(draft)}
          onChange={(checked) => change((d) => ({ ...d, github: checked }))}
        >
          <GroupLine group={inspection.github.group} manual={inspection.github.manual} />
        </CheckLine>
      </Section>

      <Section
        title="Repositories"
        description="Setup clones each selected repository to its folder on the new machine."
      >
        {repositories.length === 0 && !repositoriesError ? (
          <p className="text-sm text-muted-foreground">No repository is registered here.</p>
        ) : null}
        {repositories.map((repository) => (
          <RepositoryLine
            key={repository.id}
            repository={repository}
            draft={draft}
            change={change}
          />
        ))}
        {repositoriesError}
      </Section>

      {signIns.length ? (
        <Section title="Sign-ins" description="The selected agents and tools need these sign-ins.">
          {signIns.map((signIn) => (
            <SignInLine
              key={signIn.id}
              signIn={signIn}
              choice={signInChoice(draft, signIn)}
              onChange={(choice) =>
                change((d) => ({ ...d, signIns: { ...d.signIns, [signIn.id]: choice } }))
              }
            />
          ))}
        </Section>
      ) : null}

      <Advanced>
        <TextField
          label="Remote socket"
          help="The socket of the daemon on the new machine. Leave empty for the default."
          placeholder="~/.ariadne/ariadne.sock"
          mono
          value={draft.remote}
          onChange={(remote) => change((d) => ({ ...d, remote }))}
        />
        <TextField
          label="CLI program path"
          help="An ariadne program on this machine to copy. Fill both paths, or neither."
          placeholder="/usr/local/bin/ariadne"
          mono
          value={draft.cli}
          onChange={(cli) => change((d) => ({ ...d, cli }))}
        />
        <TextField
          label="Daemon program path"
          help="An ariadned program on this machine to copy."
          placeholder="/usr/local/bin/ariadned"
          mono
          value={draft.daemon}
          onChange={(daemon) => change((d) => ({ ...d, daemon }))}
        />
        <TextField
          label="Release repository"
          help="The GitHub repository of a release to install. Fill it with the tag, or neither."
          placeholder="owner/repository"
          mono
          value={draft.releaseRepository}
          onChange={(releaseRepository) => change((d) => ({ ...d, releaseRepository }))}
        />
        <TextField
          label="Release tag"
          help="The tag of that release."
          placeholder="v1.0.0"
          mono
          value={draft.releaseTag}
          onChange={(releaseTag) => change((d) => ({ ...d, releaseTag }))}
        />
      </Advanced>
    </>
  )
}

function RepositoryLine({
  repository,
  draft,
  change,
}: {
  repository: RepositoryDto
  draft: MachineDraft
  change: (patch: (draft: MachineDraft) => MachineDraft) => void
}) {
  const name = repositoryName(repository)
  const selected = repositorySelected(draft, repository)
  const set = (patch: { selected?: boolean; destination?: string }) =>
    change((d) => ({
      ...d,
      repositories: {
        ...d.repositories,
        [repository.id]: { ...d.repositories[repository.id], ...patch },
      },
    }))
  return (
    <CheckLine label={name} checked={selected} onChange={(checked) => set({ selected: checked })}>
      <span className="flex flex-col gap-1.5">
        <span className="font-mono text-xs break-all">{repository.path}</span>
        {selected ? (
          <Input
            aria-label={`Destination for ${name}`}
            className="font-mono"
            spellCheck={false}
            autoComplete="off"
            value={destinationOf(draft, repository)}
            onChange={(event) => set({ destination: event.target.value })}
          />
        ) : null}
      </span>
    </CheckLine>
  )
}

function SignInLine({
  signIn,
  choice,
  onChange,
}: {
  signIn: Inspection["sign_ins"][number]
  choice: SignInChoice
  onChange: (choice: SignInChoice) => void
}) {
  const id = useId()
  return (
    <div className="flex flex-col gap-2">
      <span id={`${id}-name`} className="text-sm">
        {signIn.name}
      </span>
      <RadioGroup
        aria-labelledby={`${id}-name`}
        value={choice}
        onValueChange={(value) => onChange(value as SignInChoice)}
        className="gap-2 pl-1"
      >
        <div className="flex items-start gap-3">
          <RadioGroupItem
            value="new"
            id={`${id}-new`}
            aria-labelledby={`${id}-new-label`}
            className="mt-0.5"
          />
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <FieldLabel id={`${id}-new-label`} htmlFor={`${id}-new`}>
              Sign in on the new machine
            </FieldLabel>
            {choice === "new" && signIn.command ? (
              <div className="text-sm text-muted-foreground">
                <span>After setup, run on the new machine:</span>
                <CommandLine command={signIn.command} />
              </div>
            ) : null}
          </div>
        </div>
        <div className="flex items-start gap-3">
          <RadioGroupItem
            value="copy"
            id={`${id}-copy`}
            aria-labelledby={`${id}-copy-label`}
            aria-describedby={signIn.copy_supported ? undefined : `${id}-copy-line`}
            disabled={!signIn.copy_supported}
            className="mt-0.5"
          />
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <FieldLabel
              id={`${id}-copy-label`}
              htmlFor={`${id}-copy`}
              className={signIn.copy_supported ? undefined : "text-muted-foreground"}
            >
              Copy from this machine
            </FieldLabel>
            {signIn.copy_supported ? null : (
              <span id={`${id}-copy-line`} className="text-sm text-muted-foreground">
                Ariadne cannot copy the {signIn.name} sign-in.
              </span>
            )}
          </div>
        </div>
      </RadioGroup>
    </div>
  )
}

/** The inputs that a default can replace, out of sight until the user opens them. */
export function Advanced({ open, children }: { open?: boolean; children: ReactNode }) {
  return (
    <details open={open || undefined} className="rounded-md border px-3 py-2">
      <summary className="cursor-pointer text-sm font-medium outline-none focus-visible:ring-3 focus-visible:ring-ring/50">
        Advanced
      </summary>
      <div className="mt-3 flex flex-col gap-4">{children}</div>
    </details>
  )
}

/** One text input with its plain label and one line of help. */
export function TextField({
  label,
  help,
  placeholder,
  value,
  onChange,
  error,
  mono = false,
  autoFocus,
}: {
  label: string
  help: string
  placeholder?: string
  value: string
  onChange: (value: string) => void
  /** A refusal of the daemon that names this field. */
  error?: { message?: string }
  mono?: boolean
  /** The first field of a step takes the keyboard when the step opens. */
  autoFocus?: boolean
}) {
  const id = useId()
  return (
    <Field data-invalid={error ? true : undefined}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        autoComplete="off"
        spellCheck={false}
        autoFocus={autoFocus}
        className={mono ? "font-mono" : undefined}
        placeholder={placeholder}
        aria-invalid={error ? true : undefined}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
      {error ? <FieldError errors={[error]} /> : <FieldDescription>{help}</FieldDescription>}
    </Field>
  )
}

const GROUPS: { group: SetupGroup; title: string }[] = [
  { group: "ariadne_installs", title: "Ariadne installs" },
  { group: "already_present", title: "Already on the machine" },
  { group: "manual", title: "You install by hand" },
]

export function ReviewStep({
  review,
  draft,
  inspection,
}: {
  review: SetupReview
  draft: MachineDraft
  inspection: Inspection
}) {
  const copied = copiedSignIns(draft, inspection)
  return (
    <>
      {GROUPS.map(({ group, title }) => {
        const items = review.plan.filter((item) => item.group === group)
        return (
          <section key={group} aria-label={title} className="flex flex-col gap-2">
            <h3 className="text-sm font-medium">{title}</h3>
            {items.length === 0 ? (
              <p className="text-sm text-muted-foreground">Nothing.</p>
            ) : (
              <ul className="flex flex-col gap-2">
                {items.map((item) => (
                  <li key={item.id} className="flex flex-col gap-0.5 text-sm">
                    <span>{item.title}</span>
                    <span className="text-muted-foreground">{item.detail}</span>
                    {item.manual ? (
                      <span className="flex flex-col gap-1 text-muted-foreground">
                        <span>{item.manual.reason}</span>
                        {item.manual.command ? <CommandLine command={item.manual.command} /> : null}
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </section>
        )
      })}
      <p className="text-sm">
        {copied.length
          ? `Setup copies these sign-ins from this machine: ${copied.map((s) => s.name).join(", ")}.`
          : "Setup copies no sign-in from this machine."}
      </p>
    </>
  )
}
