/**
 * One profile, edited in place: the detail half of the profiles screen.
 *
 * Everything a profile holds is one form — its name, what it runs on, and
 * every prompt it is spawned with, the system prompt first and then the
 * briefings of its role as tabs — with one Save for the whole of it. The bar
 * that saves is only there while something is unsaved, pinned to the bottom of
 * the pane so a long briefing can be scrolled without losing it; Discard puts
 * the form back to what is stored.
 *
 * Which briefings there are is not decided here: `GET /v1/profiles/{id}/prompts`
 * answers exactly the kinds the profile's role owns, in briefing order, and
 * each says whether its text is the profile's own or the default of the kind
 * — the daemon's flag, never a comparison made here. The form is not mounted
 * until that answer is in: a profile without its briefings is half a form, and
 * the answer is a local round trip away.
 *
 * Restoring a default is the one thing written the moment it is asked for:
 * the default is the daemon's text and no form holds a copy of it, so the
 * answer is both the write and the only way to read what it put back. It asks
 * first, because every other edit here is undone by Discard and this one is
 * not. The baseline moves with it (see `use-profile-save.ts`), so the Save
 * that follows does not write the default straight back as a text of the
 * profile's own.
 *
 * Unsaved edits are guarded on the way out. Every way of leaving this profile
 * is a navigation — another profile picked in the list, Back, a link followed
 * from the palette, another screen — and the app's router is a data router,
 * so one `useBlocker` covers all of them with one question. A panel opened
 * *over* the screen (`?task=`) is not leaving, and is let through.
 *
 * A profile updated elsewhere — the CLI, another window — arrives off the
 * event stream as a new `profile` prop, and refills the form only while it is
 * clean: an edit in progress is never overwritten by a refetch.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon, Trash2Icon, Undo2Icon } from "lucide-react"
import { type ReactNode, type RefObject, useCallback, useEffect, useRef, useState } from "react"
import { type Control, Controller, type FieldPath, useForm } from "react-hook-form"
import { type Location, useBlocker } from "react-router-dom"

import type { ModelDto, ProfileDto, ProfilePromptDto, PromptKind } from "@/api"
import { ConfirmDialog } from "@/components/confirm-dialog"
import { ErrorState } from "@/components/error-state"
import { submitOnChord } from "@/components/form-dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Textarea } from "@/components/ui/textarea"
import { When } from "@/components/when"
import { cn, describeError } from "@/lib/format"
import { PROFILE_PARAM } from "@/routes/paths"

import { DeleteProfileDialog } from "./delete-profile-dialog"
import { LoadingPrompts } from "./loading-prompts"
import { PinPicker } from "./pin-picker"
import {
  type ProfileFormValues,
  profileFormSchema,
  profileToFormValues,
} from "./profile-form-values"
import { PROMPT_KIND_HINTS, PROMPT_KIND_LABELS, roleLabel } from "./profile-labels"
import {
  modelsQueryOptions,
  profilePromptsQueryOptions,
  useResetProfilePrompt,
  useResetSystemPrompt,
} from "./queries"
import { useProfileSave } from "./use-profile-save"

/** The tab of the system prompt, which is not one of the briefing kinds. */
const SYSTEM_PROMPT_TAB = "system_prompt"

/** What an unpinned profile runs on, said in the picker where nothing is pinned. */
const UNPINNED_LABEL = "auto — first installed CLI, on its own default model"

export function ProfileEditor({
  profile,
  onBack,
  onDeleted,
}: {
  profile: ProfileDto
  /** The way back to the list, where the list is not beside the editor. */
  onBack: () => void
  /** The profile is gone: the selection has nothing to show any more. */
  onDeleted: () => void
}) {
  const [deleteOpen, setDeleteOpen] = useState(false)
  // A profile being deleted is not one whose edits are worth a question: the
  // guard reads this before it asks, since the delete's own confirmation has
  // already been answered.
  const leaving = useRef(false)

  // What this profile is briefed with: its own text where it has one, the
  // default of the kind where it has none. Not refetched by `profile_updated`
  // (see `queries.ts`), so nothing swaps the array under an editor.
  const stored = useQuery(profilePromptsQueryOptions(profile.id))
  // The catalog behind the pin picker, allowed to fail: an undefined catalog
  // leaves the field free-text.
  const models = useQuery(modelsQueryOptions())

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1">
        <Button
          variant="ghost"
          size="icon-sm"
          className="-ml-1.5 md:hidden"
          aria-label="Back to the list"
          onClick={onBack}
        >
          <ArrowLeftIcon />
        </Button>
        {/* The stored name, not the box's: the title says which profile this
            is, and a rename is not one until it is saved. */}
        <h2 className="min-w-0 truncate font-heading text-base font-semibold">{profile.name}</h2>
        <Badge variant="secondary">{roleLabel(profile.role)}</Badge>
        <p className="text-xs text-muted-foreground">
          Created <When at={profile.created_at} label="created" /> · updated{" "}
          <When at={profile.updated_at} label="updated" />
        </p>
        <Button variant="outline" size="sm" className="ml-auto" onClick={() => setDeleteOpen(true)}>
          <Trash2Icon />
          Delete
        </Button>
      </header>

      {stored.isPending ? (
        <LoadingPrompts />
      ) : stored.isError ? (
        <ErrorState
          title="Could not load the prompts"
          error={stored.error}
          onRetry={() => void stored.refetch()}
        />
      ) : (
        <ProfileForm
          profile={profile}
          prompts={stored.data}
          models={models.data}
          leaving={leaving}
        />
      )}

      <DeleteProfileDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        profile={profile}
        onDeleted={() => {
          leaving.current = true
          onDeleted()
        }}
      />
    </div>
  )
}

/**
 * Whether a navigation leaves the profile being edited: another screen, or
 * another selection on this one. A panel opened over the screen changes
 * neither, and the editor stays mounted under it with its edits intact.
 */
function leavesProfile(current: Location, next: Location): boolean {
  if (current.pathname !== next.pathname) return true
  return (
    new URLSearchParams(current.search).get(PROFILE_PARAM) !==
    new URLSearchParams(next.search).get(PROFILE_PARAM)
  )
}

function ProfileForm({
  profile,
  prompts,
  models,
  leaving,
}: {
  profile: ProfileDto
  /** The briefings as stored, which is what the form is filled with and diffed against. */
  prompts: ProfilePromptDto[]
  models: ModelDto[] | undefined
  /** Set by the editor once the profile is being deleted, so the guard stands down. */
  leaving: RefObject<boolean>
}) {
  // Filled once, at mount: what the daemon held when this profile was opened.
  // Later stored states come in through `reseed` below rather than through
  // the form's defaults, which is what keeps an edit in progress intact.
  const [initial] = useState(() => profileToFormValues(profile, prompts))
  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: initial,
  })
  const { control, formState, handleSubmit, register, setError, setValue, watch } = form

  /**
   * Whether the profile is on its role's default system prompt.
   *
   * State rather than the prop alone because a restore and a save both change
   * it before the list behind the prop has refetched.
   */
  const [systemIsDefault, setSystemIsDefault] = useState(profile.system_prompt_is_default)
  useEffect(() => {
    setSystemIsDefault(profile.system_prompt_is_default)
  }, [profile.system_prompt_is_default])

  const save = useProfileSave(profile, form, initial, {
    onProfileSaved: (updated) => setSystemIsDefault(updated.system_prompt_is_default),
  })
  const { dirty, reseed } = save

  // A profile or a briefing updated elsewhere refills the form — but only
  // while it is clean. The effect answers to the data alone; what it does
  // with it reads the latest render through a ref, so a keystroke does not
  // re-run it.
  const latest = useRef({ dirty, reseed })
  latest.current = { dirty, reseed }
  const seededFrom = useRef({ profile, prompts })
  useEffect(() => {
    if (seededFrom.current.profile === profile && seededFrom.current.prompts === prompts) return
    seededFrom.current = { profile, prompts }
    if (latest.current.dirty) return
    latest.current.reseed(profileToFormValues(profile, prompts))
  }, [profile, prompts])

  // Leaving the profile with edits on it asks first. The question is the
  // router's to raise, since every way out is a navigation; answering it is
  // the dialog below.
  const blocker = useBlocker(
    useCallback(
      ({ currentLocation, nextLocation }: { currentLocation: Location; nextLocation: Location }) =>
        dirty && !leaving.current && leavesProfile(currentLocation, nextLocation),
      [dirty, leaving],
    ),
  )

  const resetPrompt = useResetProfilePrompt()
  const resetSystemPrompt = useResetSystemPrompt()
  /**
   * A restore is in flight. Save waits for it, the way a restore waits for a
   * save: a save started now would snapshot the text the restore is about to
   * replace, and once the restore had moved the baseline that snapshot would
   * read as an edit and be written straight back over the default.
   */
  const restoring = resetPrompt.isPending || resetSystemPrompt.isPending

  /**
   * Drop the text set on one briefing and fill its editor with the default
   * that takes over — a write of its own, since the default is the daemon's to
   * say. The baseline moves with it, so the Save that follows does not write
   * the default straight back as a text of the profile's own.
   */
  async function restore(kind: PromptKind) {
    try {
      const prompt = await resetPrompt.mutateAsync({ id: profile.id, kind })
      save.promptStored({ kind, content: prompt.content })
    } catch (error) {
      setError("root", {
        message: `The ${PROMPT_KIND_LABELS[kind].toLowerCase()} could not be restored: ${describeError(error)}`,
      })
    }
  }

  /** The same for the system prompt, which lives on the profile itself. */
  async function restoreSystemPrompt() {
    try {
      const updated = await resetSystemPrompt.mutateAsync(profile.id)
      save.systemPromptStored(updated.system_prompt)
      setSystemIsDefault(true)
    } catch (error) {
      setError("root", {
        message: `The system prompt could not be restored: ${describeError(error)}`,
      })
    }
  }

  /** Whether the profile has no text of its own for one briefing. */
  function isDefault(kind: PromptKind): boolean {
    return prompts.find((prompt) => prompt.kind === kind)?.is_default ?? true
  }

  const [tab, setTab] = useState<string>(SYSTEM_PROMPT_TAB)
  const effort = watch("effort")
  const model = watch("model")
  const systemPrompt = watch("systemPrompt")
  const briefings = watch("prompts")
  // What the pinned model can do, when the catalog knows it: the id carries
  // the agent CLI, so it is the whole key.
  const catalogModel = models?.find((entry) => entry.id === model.trim())

  return (
    // The form is the pane's scrollport, so the bar at its end can stick to
    // its bottom edge. `contain-paint` keeps what it hides from counting as
    // overflow for the shell's `<main>` above it, which would otherwise grow a
    // second scrollbar behind this one (see `goal-swimlanes.tsx`).
    <form
      // The guard on the handler as well as on the button: a submit can also
      // come from the chord or from Enter in the name box, and neither
      // consults a disabled button.
      onSubmit={handleSubmit((values) => {
        if (restoring) return
        void save.save(values)
      })}
      onKeyDown={save.saving || restoring ? undefined : submitOnChord}
      className="flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto contain-paint"
      aria-label={`Edit ${profile.name}`}
    >
      <FieldGroup className="px-px pb-4">
        {/* Side by side where the *pane* is wide enough — a container query,
            not a viewport one: at the window's minimum width the pane beside
            the list is narrower than a phone, whatever the viewport says. */}
        <div className="flex flex-col gap-5 @md/field-group:flex-row @md/field-group:gap-4">
          <Field
            className="@md/field-group:w-72"
            data-invalid={formState.errors.name ? true : undefined}
          >
            <FieldLabel htmlFor="profile-name">Name</FieldLabel>
            <Input
              id="profile-name"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={formState.errors.name ? true : undefined}
              {...register("name")}
            />
            {formState.errors.name ? (
              <FieldError errors={[formState.errors.name]} />
            ) : (
              <FieldDescription>
                Unique. Anywhere a profile id is accepted, this name is too.
              </FieldDescription>
            )}
          </Field>

          <Field
            className="@md/field-group:flex-1"
            data-invalid={formState.errors.model ? true : undefined}
          >
            <FieldLabel htmlFor="profile-pin">Runs on</FieldLabel>
            <Controller
              control={control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="profile-pin"
                  label="Runs on"
                  model={field.value}
                  effort={effort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models}
                  invalid={formState.errors.model ? true : undefined}
                  // Nothing stands behind a profile the way a profile stands
                  // behind a task's slots: its empty is auto and nothing else.
                  unpinnedLabel={UNPINNED_LABEL}
                />
              )}
            />
            {formState.errors.model ? (
              <FieldError errors={[formState.errors.model]} />
            ) : (
              // The catalog's own word on the pinned model where it has one;
              // otherwise what the field is.
              <FieldDescription>
                {catalogModel?.description ??
                  "The agent CLI and, after a “:”, the model of it, with the effort that model is run at. Empty is auto: the first installed CLI, on its own default model."}
              </FieldDescription>
            )}
          </Field>
        </div>

        <Field>
          {/* A heading rather than a label: what follows is a strip of tabs,
              not one control to point a `for` at. */}
          <FieldTitle>Prompts</FieldTitle>
          <FieldDescription>
            What a {roleLabel(profile.role).toLowerCase()} is spawned with — its own text where it
            has one, its role's default where it has none. Saved with the rest of the form;
            restoring a default is written straight away.
          </FieldDescription>
          <Tabs value={tab} onValueChange={(next) => setTab(String(next))}>
            {/* Six tabs do not fit a narrow pane on one line, so the strip
                wraps — which means its height is its content's, not the fixed
                row the primitive sizes itself and its triggers to. */}
            <TabsList
              variant="line"
              className="w-full flex-wrap justify-start gap-x-1 gap-y-1 border-b pb-1 group-data-horizontal/tabs:h-auto"
              aria-label="Prompts"
            >
              <PromptTab
                value={SYSTEM_PROMPT_TAB}
                label="System prompt"
                dirty={systemPrompt !== save.stored.systemPrompt}
              />
              {briefings.map((prompt, index) => (
                <PromptTab
                  key={prompt.kind}
                  value={prompt.kind}
                  label={PROMPT_KIND_LABELS[prompt.kind]}
                  dirty={prompt.content !== save.stored.prompts[index]?.content}
                />
              ))}
            </TabsList>

            <TabsContent value={SYSTEM_PROMPT_TAB}>
              <PromptEditor
                control={control}
                name="systemPrompt"
                label="System prompt"
                hint="Prepended to whatever Ariadne tells the agent about its task."
                isDefault={systemIsDefault}
                restoring={resetSystemPrompt.isPending}
                saving={save.saving}
                onRestore={() => void restoreSystemPrompt()}
              />
            </TabsContent>
            {briefings.map((prompt, index) => (
              <TabsContent key={prompt.kind} value={prompt.kind}>
                <PromptEditor
                  control={control}
                  name={`prompts.${index}.content`}
                  label={PROMPT_KIND_LABELS[prompt.kind]}
                  hint={PROMPT_KIND_HINTS[prompt.kind]}
                  isDefault={isDefault(prompt.kind)}
                  restoring={resetPrompt.isPending}
                  saving={save.saving}
                  onRestore={() => void restore(prompt.kind)}
                />
              </TabsContent>
            ))}
          </Tabs>
        </Field>
      </FieldGroup>

      {/* Only while there is something to save. `mt-auto` seats it at the
          bottom of a pane the form does not fill; `sticky` keeps it there once
          the form is longer than the pane. */}
      {dirty ? (
        <div className="sticky bottom-0 mt-auto flex flex-col gap-2 border-t bg-background pt-3">
          {formState.errors.root ? (
            <ErrorState
              title="Could not save the profile"
              error={null}
              description={formState.errors.root.message}
            />
          ) : null}
          <div className="flex items-center gap-2">
            <p className="text-sm text-muted-foreground">
              {save.saving ? "Saving…" : restoring ? "Restoring…" : "Unsaved changes"}
            </p>
            <Button
              type="button"
              variant="outline"
              className="ml-auto"
              disabled={save.saving || restoring}
              onClick={save.discard}
            >
              Discard
            </Button>
            <Button type="submit" pending={save.saving} disabled={restoring}>
              Save
            </Button>
          </div>
        </div>
      ) : null}

      {/* The router's question, in the app's own words: the same ones a
          dirty form dialog asks on the way out. */}
      <ConfirmDialog
        open={blocker.state === "blocked"}
        onClose={() => blocker.reset?.()}
        title="Discard changes?"
        description="This profile has unsaved changes. Leaving it now drops them."
        confirmLabel="Discard"
        dismissLabel="Keep editing"
        destructive
        onConfirm={() => blocker.proceed?.()}
      />
    </form>
  )
}

/**
 * One prompt's tab. A tab with unsaved text wears a dot, which is also said
 * in words for whoever cannot see it.
 */
function PromptTab({ value, label, dirty }: { value: string; label: string; dirty: boolean }) {
  return (
    <TabsTrigger value={value} className="h-7 flex-none" data-dirty={dirty ? "true" : undefined}>
      {label}
      {dirty ? (
        <>
          <span aria-hidden className="size-1.5 rounded-full bg-primary" />
          <span className="sr-only">, unsaved</span>
        </>
      ) : null}
    </TabsTrigger>
  )
}

/**
 * One prompt's editor: whether the stored text is the profile's own, the box,
 * when the prompt is sent, and the way back to the default.
 *
 * The badge and the button read the *stored* state rather than what is in the
 * box: they are about what the daemon holds, which is what a restore acts on.
 */
function PromptEditor({
  control,
  name,
  label,
  hint,
  isDefault,
  restoring,
  saving,
  onRestore,
}: {
  control: Control<ProfileFormValues>
  name: FieldPath<ProfileFormValues>
  /** How the prompt is spelled on screen; also names the textarea. */
  label: string
  /** When the daemon sends this prompt, under the box. */
  hint: ReactNode
  /**
   * Whether the profile runs on the default of this prompt rather than on a
   * text of its own — the daemon's word, not a comparison made here.
   */
  isDefault: boolean
  /** True while a restore is in flight. */
  restoring: boolean
  /**
   * A save is in flight. A restore is a write of its own, and one landing
   * between the save's writes would be undone by the next of them, so it
   * waits; typing does not, since what is typed is kept and saved next.
   */
  saving: boolean
  onRestore: () => void
}) {
  const [confirming, setConfirming] = useState(false)

  return (
    <div className="flex flex-col gap-2 pt-2">
      <div className="flex items-center gap-2">
        <Badge variant="outline">{isDefault ? "default" : "edited"}</Badge>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="ml-auto"
          aria-label={`Restore ${label} to default`}
          // Already on its default: there is nothing to drop.
          disabled={isDefault || restoring || saving}
          onClick={() => setConfirming(true)}
        >
          <Undo2Icon />
          Restore default
        </Button>
      </div>
      <Controller
        control={control}
        name={name}
        render={({ field }) => (
          <Textarea
            aria-label={label}
            spellCheck={false}
            className={cn("min-h-96 resize-y font-mono text-xs leading-relaxed")}
            value={typeof field.value === "string" ? field.value : ""}
            onChange={field.onChange}
            onBlur={field.onBlur}
            name={field.name}
            ref={field.ref}
          />
        )}
      />
      <FieldDescription className="text-xs">{hint}</FieldDescription>

      {/* The one control here that acts before Save, so the one that asks —
          and the question says exactly that. What the restore may fail with is
          reported where every other failure of this form lands. */}
      <ConfirmDialog
        open={confirming}
        onClose={() => setConfirming(false)}
        title={`Restore ${label.toLowerCase()} to its default?`}
        description={
          <>
            The text this profile has of its own is dropped and the{" "}
            <span className="font-medium text-foreground">{label.toLowerCase()}</span> goes back to
            the default of its role. It is written straight away: discarding the other edits
            afterwards does not put it back.
          </>
        }
        confirmLabel="Restore default"
        destructive
        onConfirm={() => {
          setConfirming(false)
          onRestore()
        }}
      />
    </div>
  )
}
