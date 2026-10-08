/**
 * The filter bar both Forge tabs share: the enabled repository a list is
 * narrowed to, and a control of the tab's own beside it — a switch, or the
 * choice between a few views. Each filter lives in the
 * tab's search params, so a narrowed list survives a reload, and picking the
 * other tab — another route — starts that one unfiltered.
 */

import { SearchIcon } from "lucide-react"
import type { ReactNode } from "react"
import { useSearchParams } from "react-router-dom"

import type { RepositoryDto } from "@/api"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"

/** What the repository filter holds for "every enabled repository". */
const ALL = "all"

/** `owner/name` for a repository on a forge, else its path. */
export function forgeName(repository: RepositoryDto | undefined): string {
  if (!repository) return ""
  return repository.forge ? `${repository.forge.owner}/${repository.forge.name}` : repository.path
}

/**
 * Whether a row answers `query`: every word of it is in one of `fields`,
 * ignoring case. An empty query answers every row.
 */
export function matchesText(query: string | null, fields: (string | number | null | undefined)[]) {
  const words = (query ?? "").toLowerCase().split(/\s+/).filter(Boolean)
  if (words.length === 0) return true
  const text = fields
    .filter((field) => field !== null && field !== undefined)
    .join("\n")
    .toLowerCase()
  return words.every((word) => text.includes(word))
}

/** One search param of the tab, read and written. */
export function useSearchFilter(key: string): [string | null, (value: string | null) => void] {
  const [search, setSearch] = useSearchParams()
  return [
    search.get(key),
    (value) => {
      const next = new URLSearchParams(search)
      if (value) next.set(key, value)
      else next.delete(key)
      setSearch(next, { replace: true })
    },
  ]
}

export function ForgeFilters({
  repositories,
  repository,
  onRepository,
  query,
  onQuery,
  placeholder,
  toggle,
  children,
}: {
  /** The repositories whose forge integration is enabled. */
  repositories: RepositoryDto[]
  /** The repository the list is narrowed to, or null for all of them. */
  repository: string | null
  onRepository: (id: string | null) => void
  /** The text the rows are narrowed to: words of their title or description. */
  query: string | null
  onQuery: (query: string | null) => void
  placeholder: string
  toggle?: { id: string; label: string; checked: boolean; onChange: (checked: boolean) => void }
  /** A control of the tab's own, after the repository. */
  children?: ReactNode
}) {
  const items = [
    { value: ALL, label: "All repositories" },
    ...repositories.map((each) => ({ value: each.id, label: forgeName(each) })),
  ]
  return (
    <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
      <div className="relative w-72">
        <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          type="search"
          aria-label="Filter by text"
          placeholder={placeholder}
          value={query ?? ""}
          onChange={(event) => onQuery(event.target.value || null)}
          className="pl-8"
        />
      </div>
      <Select
        value={repository ?? ALL}
        onValueChange={(value) => onRepository(value === ALL ? null : (value as string))}
        items={items}
      >
        <SelectTrigger aria-label="Filter by repository" className="w-56">
          <SelectValue />
        </SelectTrigger>
        <SelectContent alignItemWithTrigger={false} className="w-72">
          {items.map((item) => (
            <SelectItem key={item.value} value={item.value}>
              {item.value === ALL ? (
                item.label
              ) : (
                <span className="truncate font-mono text-xs">{item.label}</span>
              )}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {children}
      {toggle ? (
        <div className="flex items-center gap-2">
          <Switch id={toggle.id} checked={toggle.checked} onCheckedChange={toggle.onChange} />
          <Label htmlFor={toggle.id} className="text-sm font-normal">
            {toggle.label}
          </Label>
        </div>
      ) : null}
    </div>
  )
}
