import { useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"

const KEY = "ariadne.goal-views"
const FIELDS = ["q", "repo", "status", "presentation"]
type View = { name: string; scope: string }

function readViews(): View[] {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]")
    return Array.isArray(data)
      ? data.filter(
          (row): row is View => typeof row?.name === "string" && typeof row?.scope === "string",
        )
      : []
  } catch {
    return []
  }
}

export function SavedViews({
  search,
  apply,
}: {
  search: URLSearchParams
  apply: (next: URLSearchParams) => void
}) {
  const [views, setViews] = useState(readViews)
  const [selected, setSelected] = useState("")
  const [name, setName] = useState("")
  const [error, setError] = useState("")
  function store(next: View[]) {
    try {
      localStorage.setItem(KEY, JSON.stringify(next))
      setViews(next)
      setError("")
    } catch {
      setError("Local views could not be saved.")
    }
  }
  return (
    <div className="flex flex-wrap items-center gap-2">
      <select
        aria-label="Saved local views"
        className="h-9 max-w-48 rounded-md border bg-background px-2 text-sm"
        value={selected}
        onChange={(event) => {
          const value = event.target.value
          setSelected(value)
          const view = views.find((row) => row.name === value)
          if (!view) return
          setName(view.name)
          const next = new URLSearchParams(search)
          const scope = new URLSearchParams(view.scope)
          for (const field of FIELDS) next.set(field, scope.get(field) ?? "")
          apply(next)
        }}
      >
        <option value="">Saved local views</option>
        {views.map((view) => (
          <option key={view.name}>{view.name}</option>
        ))}
      </select>
      <Input
        aria-label="Local view name"
        placeholder="View name"
        className="w-36"
        value={name}
        onChange={(event) => setName(event.target.value)}
      />
      <Button
        variant="outline"
        disabled={!name.trim() || views.some((view) => view.name === name.trim())}
        onClick={() => {
          const scope = new URLSearchParams()
          for (const field of FIELDS) scope.set(field, search.get(field) ?? "")
          store([...views, { name: name.trim(), scope: scope.toString() }])
          setSelected(name.trim())
        }}
      >
        Save view
      </Button>
      <Button
        variant="outline"
        disabled={
          !selected ||
          !name.trim() ||
          (name.trim() !== selected && views.some((view) => view.name === name.trim()))
        }
        onClick={() => {
          store(
            views.map((view) => (view.name === selected ? { ...view, name: name.trim() } : view)),
          )
          setSelected(name.trim())
        }}
      >
        Rename view
      </Button>
      <Button
        variant="outline"
        disabled={!selected}
        onClick={() => {
          store(views.filter((view) => view.name !== selected))
          setSelected("")
        }}
      >
        Delete view
      </Button>
      {error ? <p role="alert">{error}</p> : null}
    </div>
  )
}
