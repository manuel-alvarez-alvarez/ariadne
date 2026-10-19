/**
 * A dashboard of your own: a name, and what it starts from.
 *
 * "Empty" is a valid, empty Perses dashboard (036) — a blank canvas to build
 * panels on. "Copy" takes the picked dashboard's own panels, layouts,
 * variables and time settings and files them under the new name, a starting
 * point rather than a blank one. Offered only while a dashboard is picked to
 * copy from, which the Stats screen always has — the shipped `stats`
 * dashboard is never gone.
 *
 * Validation is client-side for the name and daemon-side for what it alone
 * knows: a name already taken comes back as a 409 and is shown on the name
 * field.
 */

import { useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type DashboardDto } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  useResetOnOpen,
} from "@/components/form-dialog"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { describeError } from "@/lib/format"

import { copyDashboardDocument, emptyDashboardDocument } from "./document"
import { useCreateDashboard } from "./queries"

type NewDashboardFormValues = { name: string; source: "empty" | "copy" }

const EMPTY: NewDashboardFormValues = { name: "", source: "empty" }

export function NewDashboardDialog({
  open,
  onOpenChange,
  current,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** What a "Copy of …" pick starts from; omitted hides that choice. */
  current?: DashboardDto
  onCreated?: (dashboard: DashboardDto) => void
}) {
  const createDashboard = useCreateDashboard()

  const form = useForm<NewDashboardFormValues>({ defaultValues: EMPTY })
  const { formState, handleSubmit, register, setError } = form
  useResetOnOpen(open, form, EMPTY, createDashboard)

  async function submit(values: NewDashboardFormValues) {
    const named = values.name.trim()
    const document =
      values.source === "copy" && current
        ? copyDashboardDocument(current.document, named)
        : emptyDashboardDocument(named)
    try {
      const created = await createDashboard.mutateAsync({ name: named, document })
      toast.success("Dashboard created", { description: created.name })
      onOpenChange(false)
      onCreated?.(created)
    } catch (error) {
      if (ApiError.is(error) && error.status === 409) {
        setError("name", { message: `A dashboard named "${named}" already exists.` })
        return
      }
      setError("root", { message: describeError(error) })
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        title="New dashboard"
        description="A dashboard of your own, picked by the same picker as the ones Ariadne ships."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: "Could not create the dashboard",
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel="Create dashboard"
        pending={createDashboard.isPending}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="new-dashboard-name">Name</FieldLabel>
            <Input
              id="new-dashboard-name"
              placeholder="on-call"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={formState.errors.name ? true : undefined}
              {...register("name", {
                validate: (value) => value.trim().length > 0 || "A dashboard needs a name.",
              })}
            />
            {formState.errors.name ? <FieldError errors={[formState.errors.name]} /> : null}
          </Field>

          <div role="radiogroup" aria-label="Starts from" className="flex flex-col gap-2">
            <label className="flex items-center gap-2 text-sm">
              <input type="radio" value="empty" {...register("source")} />
              Empty
            </label>
            {current ? (
              <label className="flex items-center gap-2 text-sm">
                <input type="radio" value="copy" {...register("source")} />
                {`Copy of ${current.name}`}
              </label>
            ) : null}
          </div>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
