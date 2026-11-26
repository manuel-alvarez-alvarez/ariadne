/**
 * "New task": a task with no goal, created straight from the app.
 *
 * The repository is required and the workflow starts on "No workflow". With a
 * workflow the dialog staffs one row per column; without one it staffs the one
 * agent that does the whole task. There is no dependency field: a goal-less
 * task depends on nothing. The host picker offers the ready hosts and starts on
 * this machine. The repositories, workflows and models are `local`'s, which is
 * the daemon the task is created on.
 */

import { zodResolver } from "@hookform/resolvers/zod"
import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { Controller, useForm } from "react-hook-form"
import { toast } from "sonner"

import { ApiError, LOCAL_HOST, type TaskDto, useHosts } from "@/api"
import {
  FormDialog,
  FormDialogBody,
  FormDialogContent,
  submitOnChord,
  useClearErrorOnEdit,
  useResetOnOpen,
} from "@/components/form-dialog"
import { FormSelect } from "@/components/form-select"
import { HostDot } from "@/components/host-dot"
import { MarkdownField } from "@/components/markdown-field"
import { Field, FieldDescription, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { PinPicker } from "@/features/models/pin-picker"
import { modelsQueryOptions } from "@/features/models/queries"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { skillsQueryOptions } from "@/features/skills/queries"
import { SkillsInput } from "@/features/skills/skills-input"
import { workflowsQueryOptions } from "@/features/workflows/queries"
import { useCreateStandaloneTask } from "./queries"
import {
  NO_WORKFLOW,
  type StandaloneTaskValues,
  staffingRows,
  standaloneTaskSchema,
  toCreateStandaloneTaskRequest,
} from "./standalone-task-values"

const DEFAULTS: StandaloneTaskValues = {
  title: "",
  description: "",
  repo_id: "",
  workflow: NO_WORKFLOW,
  host: LOCAL_HOST,
  agents: staffingRows(null),
}

export function CreateStandaloneTaskDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated?: (task: TaskDto) => void
}) {
  const { hosts } = useHosts()
  const repositories = useQuery({ ...repositoriesQueryOptions(LOCAL_HOST), enabled: open })
  const workflows = useQuery({ ...workflowsQueryOptions(LOCAL_HOST), enabled: open })
  const models = useQuery({ ...modelsQueryOptions(LOCAL_HOST), enabled: open })
  const skills = useQuery({ ...skillsQueryOptions(), enabled: open })
  const create = useCreateStandaloneTask()
  const form = useForm<StandaloneTaskValues>({
    resolver: zodResolver(standaloneTaskSchema),
    defaultValues: DEFAULTS,
  })
  useResetOnOpen(open, form, DEFAULTS, create)
  useClearErrorOnEdit(form, create)
  const errors = form.formState.errors
  const agentValues = form.watch("agents")
  const workflow = form.watch("workflow")
  const catalog = workflows.data ?? []
  const skillNames = useMemo(
    () => (skills.data ?? []).filter((skill) => skill.seat === "task").map((skill) => skill.name),
    [skills.data],
  )
  const steps = catalog.find((one) => one.name === workflow)?.steps ?? null
  const hasModel = agentValues.every((agent) => agent.model.trim())

  async function onSubmit(values: StandaloneTaskValues) {
    try {
      const task = await create.mutateAsync(toCreateStandaloneTaskRequest(values))
      toast.success("Task created", { description: task.title })
      onOpenChange(false)
      onCreated?.(task)
    } catch {
      // Rendered inline by the dialog: the daemon's reason is the useful part.
    }
  }

  return (
    <FormDialog open={open} onOpenChange={onOpenChange} dirty={form.formState.isDirty}>
      <FormDialogContent
        className="sm:max-w-2xl"
        title="New task"
        description="A task that belongs to no goal. Pick a workflow, or run it with none."
        onSubmit={form.handleSubmit(onSubmit)}
        error={
          ApiError.is(create.error)
            ? { title: "Could not create task", error: create.error, showIcon: true }
            : null
        }
        submitLabel="Create task"
        pending={create.isPending}
        submitDisabled={!hasModel}
        onKeyDown={submitOnChord}
      >
        <FormDialogBody>
          <Field data-invalid={errors.title ? "" : undefined}>
            <FieldLabel htmlFor="standalone-title">Title</FieldLabel>
            <Input id="standalone-title" autoComplete="off" {...form.register("title")} />
            <FieldError>{errors.title?.message}</FieldError>
          </Field>
          <Controller
            control={form.control}
            name="description"
            render={({ field }) => (
              <MarkdownField
                id="standalone-description"
                label="Description"
                description="Markdown for the agents."
                value={field.value}
                onChange={field.onChange}
                onBlur={field.onBlur}
                name={field.name}
                ref={field.ref}
              />
            )}
          />
          <Field data-invalid={errors.repo_id ? "" : undefined}>
            <FieldLabel htmlFor="standalone-repo">Repository</FieldLabel>
            <FormSelect
              control={form.control}
              name="repo_id"
              id="standalone-repo"
              options={(repositories.data ?? []).map((repo) => ({
                label: repo.path,
                value: repo.id,
              }))}
              placeholder="Select a repository"
              invalid={errors.repo_id ? true : undefined}
            />
            <FieldError>{errors.repo_id?.message}</FieldError>
          </Field>
          <Field>
            <FieldLabel htmlFor="standalone-workflow">Workflow</FieldLabel>
            <FormSelect
              control={form.control}
              name="workflow"
              id="standalone-workflow"
              options={[
                { value: NO_WORKFLOW, label: "No workflow" },
                ...catalog.map((one) => ({ value: one.name, label: one.name })),
              ]}
              onValueChange={(value) => {
                const picked = catalog.find((one) => one.name === value)
                form.setValue("agents", staffingRows(picked?.steps ?? null), {
                  shouldDirty: true,
                })
              }}
            />
            <FieldDescription>
              The columns the task moves through, one agent each. With no workflow, one agent does
              the whole task.
            </FieldDescription>
          </Field>
          <Field>
            <FieldLabel>Agents</FieldLabel>
            <div className="flex flex-col gap-3">
              {agentValues.map((agent, index) => {
                const title = steps?.find((step) => step.id === agent.step)?.title ?? "Agent"
                return (
                  <div
                    key={agent.step || "agent"}
                    className="grid gap-2 sm:grid-cols-[10rem_1fr_1fr]"
                  >
                    <span className="pt-2 text-sm font-medium">{steps ? title : "Agent"}</span>
                    <SkillsInput
                      control={form.control}
                      name={`agents.${index}.skills`}
                      ariaLabel={`${title} skills`}
                      suggestions={skillNames}
                    />
                    <Controller
                      control={form.control}
                      name={`agents.${index}.model`}
                      render={({ field }) => (
                        <PinPicker
                          host={LOCAL_HOST}
                          label={`${title} runs on`}
                          model={field.value}
                          effort={agent.effort}
                          onChange={(pin) => {
                            field.onChange(pin.model)
                            form.setValue(`agents.${index}.effort`, pin.effort, {
                              shouldDirty: true,
                            })
                          }}
                          models={models.data}
                        />
                      )}
                    />
                  </div>
                )
              })}
            </div>
          </Field>
          <Field>
            <FieldLabel htmlFor="standalone-host">Host</FieldLabel>
            <FormSelect
              control={form.control}
              name="host"
              id="standalone-host"
              empty={LOCAL_HOST}
              renderOption={(option) => (
                <span className="flex items-center gap-2">
                  <HostDot host={option.value} />
                  {option.label}
                </span>
              )}
              options={hosts
                .filter((one) => one.status === "ready")
                .map((one) => ({ value: one.name, label: one.name }))}
            />
            <FieldDescription>Where the task runs. Starts on this machine.</FieldDescription>
          </Field>
        </FormDialogBody>
      </FormDialogContent>
    </FormDialog>
  )
}
