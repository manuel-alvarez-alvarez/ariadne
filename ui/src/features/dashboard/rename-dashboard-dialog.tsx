/**
 * Renaming a dashboard: the name is the table's own key (036), so this is one
 * field, submitted as a create under the new name followed by a delete of the
 * old one (`useRenameDashboard`). Never offered for the built-in dashboard —
 * the Stats screen keeps that action out of reach itself.
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

import { dashboardDisplayName } from "./display-name"
import { useRenameDashboard } from "./queries"

type RenameDashboardFormValues = { name: string }

export function RenameDashboardDialog({
  dashboard,
  open,
  onOpenChange,
  onRenamed,
}: {
  dashboard: DashboardDto
  open: boolean
  onOpenChange: (open: boolean) => void
  onRenamed?: (dashboard: DashboardDto) => void
}) {
  const rename = useRenameDashboard()

  const defaultValues: RenameDashboardFormValues = { name: dashboard.name }
  const form = useForm<RenameDashboardFormValues>({ defaultValues })
  const { formState, handleSubmit, register, setError } = form
  useResetOnOpen(open, form, defaultValues, rename)

  async function submit(values: RenameDashboardFormValues) {
    const named = values.name.trim()
    if (named === dashboard.name) {
      onOpenChange(false)
      return
    }
    try {
      const renamed = await rename.mutateAsync({
        from: dashboard.name,
        to: named,
        document: dashboard.document,
      })
      toast.success("Dashboard renamed", { description: dashboardDisplayName(renamed) })
      onOpenChange(false)
      onRenamed?.(renamed)
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
        title={`Rename ${dashboardDisplayName(dashboard)}`}
        description="Files this dashboard's panels, layouts, variables and time settings under a new name, then removes the old one."
        onSubmit={handleSubmit(submit)}
        error={
          formState.errors.root
            ? {
                title: "Could not rename the dashboard",
                error: null,
                description: formState.errors.root.message,
              }
            : null
        }
        submitLabel="Rename"
        pending={rename.isPending}
      >
        <FormDialogBody>
          <Field data-invalid={formState.errors.name ? true : undefined}>
            <FieldLabel htmlFor="rename-dashboard-name">Name</FieldLabel>
            <Input
              id="rename-dashboard-name"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={formState.errors.name ? true : undefined}
              {...register("name", {
                validate: (value) => value.trim().length > 0 || "A dashboard needs a name.",
              })}
            />
            {formState.errors.name ? <FieldError errors={[formState.errors.name]} /> : null}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
