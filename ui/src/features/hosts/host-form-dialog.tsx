/**
 * Edit dialog for a host. It keeps the name read-only and sends only the
 * fields that changed; an emptied remote is `null`. `local` can change its
 * colour alone. A new host comes from the "Add machine" dialog
 * (`add-machine-dialog.tsx`).
 *
 * A 400 or a 409 that names a field lands on it; any other refusal shows in
 * the dialog.
 */

import { useEffect } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"

import { type HostDto, LOCAL_HOST, type PatchHost } from "@/api"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { describeError } from "@/lib/format"
import type { HostColor } from "@/lib/host-colors"
import { fieldOf, HostColorField } from "./host-fields"
import { useUpdateHost } from "./queries"

interface HostFormValues {
  name: string
  endpoint: string
  remote: string
  /** "" is the default colour. */
  color: HostColor | ""
}

const EMPTY_VALUES: HostFormValues = { name: "", endpoint: "", remote: "", color: "" }

export function HostFormDialog({
  open,
  onOpenChange,
  host,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The host being edited, or null while no row has opened the dialog. */
  host: HostDto | null
}) {
  const isLocal = host?.name === LOCAL_HOST
  const updateHost = useUpdateHost()
  const saving = updateHost.isPending

  const form = useForm<HostFormValues>({ defaultValues: EMPTY_VALUES })
  const { control, formState, handleSubmit, register, reset, setError, clearErrors } = form

  // Every open starts from what is stored, never from the previous attempt;
  // keyed off the opening so a `host_updated` mid-edit does not wipe the draft.
  useEffect(() => {
    if (!open) return
    reset(
      host
        ? {
            name: host.name,
            endpoint: host.endpoint ?? "",
            remote: host.remote ?? "",
            color: host.color ?? "",
          }
        : EMPTY_VALUES,
    )
  }, [open, host, reset])

  async function submit(values: HostFormValues) {
    const endpoint = values.endpoint.trim()
    const remote = values.remote.trim()
    if (!host) return
    try {
      const body: PatchHost = {}
      if (!isLocal && endpoint !== (host.endpoint ?? "")) body.endpoint = endpoint
      if (!isLocal && remote !== (host.remote ?? "")) body.remote = remote || null
      if (values.color !== (host.color ?? "")) body.color = values.color || null
      if (Object.keys(body).length === 0) {
        onOpenChange(false)
        return
      }
      await updateHost.mutateAsync({ name: host.name, body })
      toast.success("Host updated", { description: host.name })
      onOpenChange(false)
    } catch (error) {
      setError(fieldOf(error) ?? "root", { message: describeError(error) })
    }
  }

  const errors = formState.errors
  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-lg"
        title="Edit host"
        description={
          isLocal
            ? "local is this daemon itself: only its colour can change."
            : "A daemon the app reads, reached at its endpoint."
        }
        onSubmit={handleSubmit(submit)}
        error={
          errors.root
            ? {
                title: "Could not save the host",
                error: null,
                description: errors.root.message,
              }
            : null
        }
        submitLabel="Save changes"
        pending={saving}
      >
        <FormDialogBody>
          {isLocal ? null : (
            <Field data-invalid={errors.name ? true : undefined}>
              <FieldLabel htmlFor="host-name">Name</FieldLabel>
              <Input
                id="host-name"
                autoComplete="off"
                spellCheck={false}
                readOnly
                aria-invalid={errors.name ? true : undefined}
                {...register("name", { onChange: () => clearErrors() })}
              />
              {errors.name ? (
                <FieldError errors={[errors.name]} />
              ) : (
                <FieldDescription>A host cannot be renamed.</FieldDescription>
              )}
            </Field>
          )}

          {isLocal ? null : (
            <>
              <Field data-invalid={errors.endpoint ? true : undefined}>
                <FieldLabel htmlFor="host-endpoint">Endpoint</FieldLabel>
                <Input
                  id="host-endpoint"
                  placeholder="ssh://user@box, http://box:7676 or /path/to.sock"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono"
                  aria-invalid={errors.endpoint ? true : undefined}
                  {...register("endpoint", { onChange: () => clearErrors() })}
                />
                {errors.endpoint ? <FieldError errors={[errors.endpoint]} /> : null}
              </Field>

              <Field data-invalid={errors.remote ? true : undefined}>
                <FieldLabel htmlFor="host-remote">Remote</FieldLabel>
                <Input
                  id="host-remote"
                  placeholder="/home/me/.ariadne/ariadne.sock"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono"
                  aria-invalid={errors.remote ? true : undefined}
                  {...register("remote", { onChange: () => clearErrors() })}
                />
                {errors.remote ? (
                  <FieldError errors={[errors.remote]} />
                ) : (
                  <FieldDescription>
                    Optional. The far daemon's socket path, for an ssh:// endpoint.
                  </FieldDescription>
                )}
              </Field>
            </>
          )}

          <Controller
            control={control}
            name="color"
            render={({ field }) => (
              <HostColorField
                value={field.value}
                onChange={(value) => {
                  field.onChange(value)
                  clearErrors()
                }}
                error={errors.color}
              />
            )}
          />
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
