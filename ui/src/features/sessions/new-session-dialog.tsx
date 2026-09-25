/**
 * `ariadne session new`, as a form: a new conversation with an agent, in a
 * directory, with no goal or task behind it.
 *
 * What it runs on is the one control every other form picks a model with: the
 * agent and, after a `:`, the model of it, and the effort that model is run
 * at inside the same popover. A model is required and no effort is the
 * model's default.
 *
 * The directory is typed, because any directory will do, and the registered
 * repositories are offered as suggestions, because they are where the work
 * usually is. The daemon checks it is an absolute path to a directory that
 * exists, and its refusal shows verbatim.
 *
 * The session comes back live and waiting: what is typed first into its
 * console is its first prompt, and its title.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { Controller, useForm } from "react-hook-form"
import { z } from "zod"

import { ApiError, type NewSessionRequest, type SessionDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { modelRefField } from "@/features/models/model-ref"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { repositoriesQueryOptions } from "@/features/repositories/queries"

import { useStartSession } from "./queries"

const formSchema = z.object({
  model: modelRefField(),
  effort: z.string(),
  working_directory: z
    .string()
    .trim()
    .min(1, "Name the directory the agent works in.")
    .refine((path) => path.startsWith("/"), "Give an absolute path, starting with /."),
})

type NewSessionForm = z.infer<typeof formSchema>

const DEFAULT_VALUES: NewSessionForm = { model: "", effort: "", working_directory: "" }

/** The suggestions under the directory field, by id. */
const REPOSITORY_PATHS = "new-session-repositories"

export function NewSessionDialog({
  open,
  onOpenChange,
  onStarted,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onStarted?: (session: SessionDto) => void
}) {
  const models = useQuery({ ...modelsQueryOptions(), enabled: open })
  const repositories = useQuery({ ...repositoriesQueryOptions(), enabled: open })
  const start = useStartSession()

  const form = useForm<NewSessionForm>({
    resolver: zodResolver(formSchema),
    defaultValues: DEFAULT_VALUES,
  })
  const errors = form.formState.errors

  useResetOnOpen(open, form, DEFAULT_VALUES, start)
  useClearErrorOnEdit(form, start)

  const chosenEffort = form.watch("effort")

  async function onSubmit(values: NewSessionForm) {
    const effort = values.effort.trim()
    const body: NewSessionRequest = {
      model: values.model.trim(),
      ...(effort.length > 0 ? { effort } : {}),
      working_directory: values.working_directory.trim(),
    }
    try {
      const session = await start.mutateAsync(body)
      onOpenChange(false)
      onStarted?.(session)
    } catch {
      // Rendered inline by the dialog: the daemon's message is the useful part.
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        title="New session"
        description="A conversation with an agent in a directory, with no goal or task behind it. What you type first is its title."
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          ApiError.is(start.error)
            ? { title: "Could not start the session", error: start.error, showIcon: true }
            : null
        }
        submitLabel="Start session"
        pending={start.isPending}
        submitDisabled={form.watch("model").trim().length === 0}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <Field data-invalid={errors.model ? "" : undefined}>
            <FieldLabel htmlFor="session-pin">Runs on</FieldLabel>
            <Controller
              control={form.control}
              name="model"
              render={({ field }) => (
                <PinPicker
                  id="session-pin"
                  label="Runs on"
                  model={field.value}
                  effort={chosenEffort}
                  onChange={(pin) => {
                    field.onChange(pin.model)
                    form.setValue("effort", pin.effort, { shouldDirty: true })
                  }}
                  models={models.data}
                  invalid={errors.model ? true : undefined}
                />
              )}
            />
            {errors.model ? (
              <FieldError>{errors.model.message}</FieldError>
            ) : (
              <FieldDescription>
                The agent and, after a <code>:</code>, the model of it.
              </FieldDescription>
            )}
          </Field>

          <Field data-invalid={errors.working_directory ? "" : undefined}>
            <FieldLabel htmlFor="session-directory">Working directory</FieldLabel>
            <Input
              id="session-directory"
              autoComplete="off"
              spellCheck={false}
              placeholder="/Users/me/dev/project"
              className="font-mono text-xs"
              list={REPOSITORY_PATHS}
              aria-invalid={errors.working_directory ? true : undefined}
              {...form.register("working_directory")}
            />
            <datalist id={REPOSITORY_PATHS}>
              {(repositories.data ?? []).map((repository) => (
                <option key={repository.id} value={repository.path} />
              ))}
            </datalist>
            {errors.working_directory ? (
              <FieldError>{errors.working_directory.message}</FieldError>
            ) : (
              <FieldDescription>
                Any directory on the daemon's machine; the registered repositories are suggested.
              </FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
