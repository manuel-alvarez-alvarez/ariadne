/**
 * A form's workflow picker: every workflow in the catalog, by name.
 *
 * Every goal and repository now runs a workflow, so there is no "no
 * workflow" choice left to offer. A goal's own pick may still be left on
 * {@link INHERIT}, offered as "Repository default" where `allowInherit` is
 * set, for the one case that is still a real choice: running whichever
 * workflow the repository names, rather than a fixed one agreed here. A
 * repository has no such choice behind it — it is what every goal without
 * one of its own falls back to — so its own form passes no `allowInherit`
 * and the picker offers only real names.
 *
 * The field holds a workflow's name, the empty string, or {@link INHERIT}
 * itself, since a select cannot pick the empty string back. {@link
 * pickedWorkflow} reads either empty spelling as "ask the repository".
 */

import { useQuery } from "@tanstack/react-query"
import type { Control, FieldPath, FieldValues } from "react-hook-form"

import { FormSelect } from "@/components/form-select"

import { workflowsQueryOptions } from "./queries"

/** Not a name the catalog can hold: a workflow's name is kebab-case words. */
const INHERIT = "-"

/** The workflow a field names, or null to ask the repository for one. */
export function pickedWorkflow(value: string): string | null {
  const name = value.trim()
  return name && name !== INHERIT ? name : null
}

export function WorkflowSelect<V extends FieldValues>({
  control,
  name,
  id,
  enabled,
  allowInherit = false,
  onValueChange,
}: {
  control: Control<V>
  name: FieldPath<V>
  id: string
  /** Fetch the catalog only while the dialog holding this is open. */
  enabled: boolean
  /** Offer "Repository default" beside the catalog, for a goal's own pick. */
  allowInherit?: boolean
  onValueChange?: (value: string) => void
}) {
  const workflows = useQuery({ ...workflowsQueryOptions(), enabled })
  const options = [
    ...(allowInherit ? [{ value: INHERIT, label: "Repository default" }] : []),
    ...(workflows.data ?? []).map((workflow) => ({ value: workflow.name, label: workflow.name })),
  ]
  return (
    <FormSelect
      control={control}
      name={name}
      id={id}
      options={options}
      placeholder={allowInherit ? "Repository default" : "Pick a workflow"}
      onValueChange={onValueChange}
    />
  )
}
