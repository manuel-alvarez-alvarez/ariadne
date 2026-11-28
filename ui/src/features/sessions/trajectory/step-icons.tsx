/**
 * The icons of the turn console: the state of a step, a group or a turn, and
 * the kind of a step.
 *
 * A state that the board also shows takes the board's icon and colour
 * (`TASK_STATUS_META`): running is in progress, completed is finished. The
 * kinds of a tool call are those of the terminal console's `theme::KINDS`,
 * each with one icon, and a kind with no icon there takes one fallback.
 */

import type { LucideIcon } from "lucide-react"
import {
  ArrowLeftRightIcon,
  ArrowRightIcon,
  BrainIcon,
  CircleDashedIcon,
  CircleHelpIcon,
  CircleSlashIcon,
  DownloadIcon,
  FileTextIcon,
  FoldVerticalIcon,
  InfoIcon,
  ListIcon,
  MessageCircleQuestionIcon,
  MessageSquareIcon,
  NetworkIcon,
  OctagonAlertIcon,
  PencilIcon,
  SearchIcon,
  ShieldQuestionIcon,
  SquareTerminalIcon,
  StickyNoteIcon,
  Trash2Icon,
  TriangleAlertIcon,
  WrenchIcon,
} from "lucide-react"

import { StatusIcon } from "@/components/status-icon"

import { TASK_STATUS_META } from "../../tasks/status"

interface Look {
  icon: LucideIcon
  tone: string
}

const board = (status: keyof typeof TASK_STATUS_META): Look => ({
  icon: TASK_STATUS_META[status].icon,
  tone: TASK_STATUS_META[status].text,
})

/** The look of each state a step or a turn has. */
const STATES: Record<string, Look> = {
  pending: board("pending"),
  running: board("in_progress"),
  completed: board("finished"),
  failed: board("failed"),
  cancelled: board("cancelled"),
  interrupted: { icon: CircleSlashIcon, tone: "text-status-danger" },
  answered: board("finished"),
  declined: board("cancelled"),
  open: { icon: CircleHelpIcon, tone: "text-status-warn" },
  warning: { icon: TriangleAlertIcon, tone: "text-status-warn" },
  info: { icon: InfoIcon, tone: "text-muted-foreground" },
}

/** A state of no known look, as a subagent can carry. */
const UNKNOWN_STATE: Look = { icon: CircleDashedIcon, tone: "text-muted-foreground" }

/** The icons of the kinds of a tool call, as the terminal console's `theme::KINDS`. */
const TOOL_KINDS: Record<string, LucideIcon> = {
  execute: SquareTerminalIcon,
  read: FileTextIcon,
  edit: PencilIcon,
  delete: Trash2Icon,
  move: ArrowRightIcon,
  search: SearchIcon,
  fetch: DownloadIcon,
  think: BrainIcon,
  switch_mode: ArrowLeftRightIcon,
}

/** The icons of the steps that are not tool calls. */
const STEP_KINDS: Record<string, LucideIcon> = {
  message: MessageSquareIcon,
  thought: BrainIcon,
  permission: ShieldQuestionIcon,
  question: MessageCircleQuestionIcon,
  note: StickyNoteIcon,
  error: OctagonAlertIcon,
  subagent: NetworkIcon,
  compaction: FoldVerticalIcon,
}

/** A tool call of a kind with no icon of its own. */
const KIND_FALLBACK = WrenchIcon

/** A group of read and search calls of both kinds. */
export const GROUP_ICON = ListIcon

/**
 * The icon of a state, with its word as the name and the tooltip. A status
 * such as `answered · Allow once` takes the look of its first word.
 */
export function StateIcon({ state }: { state: string }) {
  const look = STATES[state.split(" ")[0] ?? ""] ?? UNKNOWN_STATE
  // The icon sits in a row that is a button already: the row takes the focus.
  return <StatusIcon icon={look.icon} label={state} tone={look.tone} hintTabIndex={-1} />
}

/** The icon of a kind of step: a tool call's own kind, else the kind of step. */
export function kindIcon(kind: string, tool: boolean): LucideIcon {
  if (tool) return TOOL_KINDS[kind] ?? KIND_FALLBACK
  return STEP_KINDS[kind] ?? KIND_FALLBACK
}
