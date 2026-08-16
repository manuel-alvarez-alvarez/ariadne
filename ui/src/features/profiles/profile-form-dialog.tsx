/**
 * Create and edit dialog for a profile — one form for both, because the two
 * differ only in where they post and in the one field the daemon cannot change
 * after creation (the role).
 *
 * Validation is client-side for the two required fields and daemon-side for
 * everything it alone can know: a duplicate name comes back as a 409 and is
 * shown on the name field, anything else as an alert above the buttons.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { PlusIcon, XIcon } from "lucide-react"
import { useEffect } from "react"
import { Controller, useFieldArray, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type ProfileDto, type Role } from "@/api"
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
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"

import {
  AGENT_KIND_CHOICES,
  type AgentKindChoice,
  AUTO_AGENT_KIND,
  emptyProfileFormValues,
  type ProfileFormValues,
  profileFormSchema,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"
import { agentKindLabel, ROLE_LABELS, ROLES } from "./profile-labels"
import { useCreateProfile, useUpdateProfile } from "./queries"

export function ProfileFormDialog({
  open,
  onOpenChange,
  profile,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The profile being edited, or null to create a new one. */
  profile: ProfileDto | null
}) {
  const editing = profile !== null
  const createProfile = useCreateProfile()
  const updateProfile = useUpdateProfile()
  const saving = createProfile.isPending || updateProfile.isPending

  const form = useForm<ProfileFormValues>({
    resolver: zodResolver(profileFormSchema),
    defaultValues: emptyProfileFormValues(),
  })
  const { control, formState, handleSubmit, register, reset, setError, setValue, watch } = form
  const flags = useFieldArray({ control, name: "extraFlags" })

  // Every open starts from what is actually stored, never a stale draft.
  useEffect(() => {
    if (open) {
      reset(profile ? profileToFormValues(profile) : emptyProfileFormValues())
    }
  }, [open, profile, reset])

  async function submit(values: ProfileFormValues) {
    try {
      if (profile) {
        const updated = await updateProfile.mutateAsync({
          id: profile.id,
          body: toUpdateRequest(values),
        })
        toast.success("Profile updated", { description: updated.name })
      } else {
        const created = await createProfile.mutateAsync(toCreateRequest(values))
        toast.success("Profile created", { description: created.name })
      }
      onOpenChange(false)
    } catch (error) {
      // A name clash is the one failure that belongs on a field.
      if (ApiError.is(error) && error.status === 409) {
        setError("name", { message: `A profile named "${values.name.trim()}" already exists.` })
        return
      }
      setError("root", { message: errorMessage(error) })
    }
  }

  const model = watch("model")

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <form onSubmit={handleSubmit(submit)} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{editing ? "Edit profile" : "New profile"}</DialogTitle>
            <DialogDescription>
              A profile is what an agent runs as: which CLI, which model, and the system prompt it
              is spawned with.
            </DialogDescription>
          </DialogHeader>

          <FieldGroup className="max-h-[60svh] overflow-y-auto px-px py-px">
            <Field data-invalid={formState.errors.name ? true : undefined}>
              <FieldLabel htmlFor="profile-name">Name</FieldLabel>
              <Input
                id="profile-name"
                placeholder="rust-engineer"
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

            <div className="grid gap-5 sm:grid-cols-2">
              <Controller
                control={control}
                name="role"
                render={({ field }) => (
                  <Field>
                    <FieldLabel htmlFor="profile-role">Role</FieldLabel>
                    <Select
                      value={field.value}
                      onValueChange={(value) => field.onChange(value)}
                      disabled={editing}
                    >
                      <SelectTrigger id="profile-role" className="w-full">
                        <SelectValue>{(value: Role) => ROLE_LABELS[value]}</SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {ROLES.map((role) => (
                          <SelectItem key={role} value={role}>
                            {ROLE_LABELS[role]}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      {editing
                        ? "Fixed once the profile exists."
                        : "What this profile is spawned as."}
                    </FieldDescription>
                  </Field>
                )}
              />

              <Controller
                control={control}
                name="agentKind"
                render={({ field }) => (
                  <Field>
                    <FieldLabel htmlFor="profile-agent">Agent</FieldLabel>
                    <Select value={field.value} onValueChange={(value) => field.onChange(value)}>
                      <SelectTrigger id="profile-agent" className="w-full">
                        <SelectValue>
                          {(value: AgentKindChoice) => agentKindChoiceLabel(value)}
                        </SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {AGENT_KIND_CHOICES.map((choice) => (
                          <SelectItem key={choice} value={choice}>
                            {agentKindChoiceLabel(choice)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FieldDescription>
                      {field.value === AUTO_AGENT_KIND
                        ? "The first installed CLI, resolved at spawn time."
                        : "Pinned: spawning fails if this CLI is not installed."}
                    </FieldDescription>
                  </Field>
                )}
              />
            </div>

            <Field>
              <FieldLabel htmlFor="profile-model">Model</FieldLabel>
              <div className="flex items-center gap-2">
                <Input
                  id="profile-model"
                  placeholder="Provider default"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono"
                  {...register("model")}
                />
                {model.trim().length > 0 ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => setValue("model", "", { shouldDirty: true })}
                  >
                    Use default
                  </Button>
                ) : null}
              </div>
              <FieldDescription>
                Passed to the agent CLI as-is. Empty means the provider default.
              </FieldDescription>
            </Field>

            <Field data-invalid={formState.errors.systemPrompt ? true : undefined}>
              <FieldLabel htmlFor="profile-system-prompt">System prompt</FieldLabel>
              <Textarea
                id="profile-system-prompt"
                spellCheck={false}
                className="min-h-72 resize-y font-mono text-xs leading-relaxed"
                placeholder="You are a Rust engineer working inside a dedicated git worktree…"
                aria-invalid={formState.errors.systemPrompt ? true : undefined}
                {...register("systemPrompt")}
              />
              {formState.errors.systemPrompt ? (
                <FieldError errors={[formState.errors.systemPrompt]} />
              ) : (
                <FieldDescription>
                  Prepended to whatever Ariadne tells the agent about its task.
                </FieldDescription>
              )}
            </Field>

            <Field>
              <FieldLabel>Extra flags</FieldLabel>
              {flags.fields.length > 0 ? (
                <div className="flex flex-col gap-2">
                  {flags.fields.map((flag, index) => (
                    <div key={flag.id} className="flex items-center gap-2">
                      <Input
                        aria-label={`Flag ${index + 1}`}
                        placeholder="--permission-mode=acceptEdits"
                        autoComplete="off"
                        spellCheck={false}
                        className="font-mono"
                        {...register(`extraFlags.${index}.value`)}
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
              ) : null}
              <div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => flags.append({ value: "" })}
                >
                  <PlusIcon />
                  Add flag
                </Button>
              </div>
              <FieldDescription>
                Appended to the agent CLI's argv when a session is spawned. Blank rows are dropped.
              </FieldDescription>
            </Field>
          </FieldGroup>

          {formState.errors.root ? (
            <Alert variant="destructive">
              <AlertTitle>{editing ? "Could not save" : "Could not create"} the profile</AlertTitle>
              <AlertDescription>{formState.errors.root.message}</AlertDescription>
            </Alert>
          ) : null}

          <DialogFooter>
            <DialogClose render={<Button type="button" variant="outline" disabled={saving} />}>
              Cancel
            </DialogClose>
            <Button type="submit" disabled={saving}>
              {saving ? "Saving…" : editing ? "Save changes" : "Create profile"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function agentKindChoiceLabel(choice: AgentKindChoice): string {
  return choice === AUTO_AGENT_KIND ? "Auto-resolve" : agentKindLabel(choice)
}

function errorMessage(error: unknown): string {
  if (ApiError.is(error)) return error.message
  return error instanceof Error ? error.message : String(error)
}
