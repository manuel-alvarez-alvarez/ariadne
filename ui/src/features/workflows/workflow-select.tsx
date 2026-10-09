/**
 * A form's workflow picker: every workflow in the catalog, by name, and "No
 * workflow" for a goal that runs the author and reviewer pipeline instead.
 *
 * The field holds a workflow's name, or the empty string — or {@link
 * NO_WORKFLOW}, which is what picking "No workflow" stores, since a select
 * cannot pick the empty string back. {@link pickedWorkflow} reads either as
 * none.
 */

import { useQuery } from "@tanstack/react-query"
import type { Control, FieldPath, FieldValues } from "react-hook-form"

import { FormSelect } from "@/components/form-select"

import { workflowsQueryOptions } from "./queries"

/** Not a name the catalog can hold: a workflow's name is kebab-case words. */
const NO_WORKFLOW = "-"

/** The workflow a field names, or null for none. */
export function pickedWorkflow(value: string): string | null {
  const name = value.trim()
  return name && name !== NO_WORKFLOW ? name : null
}

export function WorkflowSelect<V extends FieldValues>({
  control,
  name,
  id,
  enabled,
  onValueChange,
}: {
  control: Control<V>
  name: FieldPath<V>
  id: string
  /** Fetch the catalog only while the dialog holding this is open. */
  enabled: boolean
  onValueChange?: (value: string) => void
}) {
  const workflows = useQuery({ ...workflowsQueryOptions(), enabled })
  const options = [
    { value: NO_WORKFLOW, label: "No workflow" },
    ...(workflows.data ?? []).map((workflow) => ({ value: workflow.name, label: workflow.name })),
  ]
  return (
    <FormSelect
      control={control}
      name={name}
      id={id}
      options={options}
      placeholder="No workflow"
      onValueChange={onValueChange}
    />
  )
}
