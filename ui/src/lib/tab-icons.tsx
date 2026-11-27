/**
 * The icon of each tab concept, once for the whole app: Sessions is the same
 * icon in the task pane and in the goal pane, and a concept the sidebar also
 * names (Sessions, Forge's pull requests, Permissions) has the sidebar's icon.
 * `components/app-sidebar.tsx` imports those three from here so the two cannot
 * drift apart.
 */

import {
  ActivityIcon,
  CircleCheckIcon,
  CircleDotIcon,
  CirclePlayIcon,
  EyeIcon,
  FileTextIcon,
  GitCompareIcon,
  GitPullRequestIcon,
  GraduationCapIcon,
  HistoryIcon,
  ListChecksIcon,
  ListIcon,
  PencilIcon,
  RadioTowerIcon,
  SparklesIcon,
  TerminalIcon,
} from "lucide-react"

export const TAB_ICONS = {
  description: FileTextIcon,
  history: HistoryIcon,
  diff: GitCompareIcon,
  tasks: ListChecksIcon,
  sessions: RadioTowerIcon,
  console: TerminalIcon,
  activity: ActivityIcon,
  pullRequests: GitPullRequestIcon,
  issues: CircleDotIcon,
  learned: GraduationCapIcon,
  ai: SparklesIcon,
  write: PencilIcon,
  preview: EyeIcon,
  active: CirclePlayIcon,
  all: ListIcon,
  finished: CircleCheckIcon,
} as const
