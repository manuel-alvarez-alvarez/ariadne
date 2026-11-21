/**
 * Add and edit dialog for a host — one form for both. Adding asks for the
 * name, the endpoint, the optional SSH remote and the colour. Editing keeps
 * the name read-only and sends only the fields that changed; an emptied remote
 * is `null`. `local` can change its colour alone.
 *
 * A 400 or a 409 that names a field lands on it; any other refusal shows in
 * the dialog.
 */

import { useEffect } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, type CreateHost, type HostDto, LOCAL_HOST, type PatchHost } from "@/api"
import { FormDialog, FormDialogBody, FormDialogContent } from "@/components/form-dialog"
import { HostColorSwatch } from "@/components/host-color-swatch"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { cn, describeError } from "@/lib/format"
import { HOST_COLORS, type HostColor } from "@/lib/host-colors"
import { useCreateHost, useUpdateHost } from "./queries"

interface HostFormValues {
  name: string
  endpoint: string
  remote: string
  /** "" is the default colour. */
  color: HostColor | ""
}

const EMPTY_VALUES: HostFormValues = { name: "", endpoint: "", remote: "", color: "" }

type FieldName = keyof HostFormValues

/** The field a refusal is about, from the words the daemon names it in. */
function fieldOf(error: ApiError): FieldName | null {
  if (error.status !== 400 && error.status !== 409) return null
  if (/remote/i.test(error.message)) return "remote"
  if (/endpoint/i.test(error.message)) return "endpoint"
  if (/name/i.test(error.message)) return "name"
  if (/colou?r/i.test(error.message)) return "color"
  return null
}

export function HostFormDialog({
  open,
  onOpenChange,
  host,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** The host being edited, or null to add one. */
  host: HostDto | null
}) {
  const editing = host !== null
  const isLocal = host?.name === LOCAL_HOST
  const createHost = useCreateHost()
  const updateHost = useUpdateHost()
  const saving = createHost.isPending || updateHost.isPending

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
    try {
      if (host) {
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
      } else {
        const body: CreateHost = { name: values.name.trim(), endpoint }
        if (remote) body.remote = remote
        body.color = values.color || null
        const created = await createHost.mutateAsync(body)
        toast.success("Host added", { description: created.name })
      }
      onOpenChange(false)
    } catch (error) {
      const field = ApiError.is(error) ? fieldOf(error) : null
      setError(field ?? "root", { message: describeError(error) })
    }
  }

  const errors = formState.errors
  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-lg"
        title={editing ? "Edit host" : "Add host"}
        description={
          isLocal
            ? "local is this daemon itself: only its colour can change."
            : "A daemon the app reads, reached at its endpoint."
        }
        onSubmit={handleSubmit(submit)}
        error={
          errors.root
            ? {
                title: `Could not ${editing ? "save" : "add"} the host`,
                error: null,
                description: errors.root.message,
              }
            : null
        }
        submitLabel={editing ? "Save changes" : "Add host"}
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
                readOnly={editing}
                aria-invalid={errors.name ? true : undefined}
                {...register("name", { onChange: () => clearErrors() })}
              />
              {errors.name ? (
                <FieldError errors={[errors.name]} />
              ) : (
                <FieldDescription>
                  {editing ? "A host cannot be renamed." : "Letters, digits, - and _."}
                </FieldDescription>
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

          <Field data-invalid={errors.color ? true : undefined}>
            <FieldLabel id="host-color-label">Colour</FieldLabel>
            <Controller
              control={control}
              name="color"
              render={({ field }) => (
                <div
                  role="radiogroup"
                  aria-labelledby="host-color-label"
                  className="flex flex-wrap gap-2"
                >
                  {[{ value: "" as const, label: "Default" }, ...HOST_COLORS].map((option) => (
                    <label
                      key={option.value || "default"}
                      className={cn(
                        "inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1 text-sm has-focus-visible:ring-3 has-focus-visible:ring-ring/50",
                        field.value === option.value
                          ? "border-foreground bg-accent"
                          : "border-input text-muted-foreground",
                      )}
                    >
                      <input
                        type="radio"
                        name="host-color"
                        className="sr-only"
                        checked={field.value === option.value}
                        onChange={() => {
                          field.onChange(option.value)
                          clearErrors()
                        }}
                      />
                      {option.value ? <HostColorSwatch color={option.value} /> : null}
                      {option.label}
                    </label>
                  ))}
                </div>
              )}
            />
            {errors.color ? (
              <FieldError errors={[errors.color]} />
            ) : (
              <FieldDescription>
                Default draws the host in the colour its name gets.
              </FieldDescription>
            )}
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
