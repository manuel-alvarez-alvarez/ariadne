import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useId, useState } from "react"
import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { describeError } from "@/lib/format"

export function WorkerSetup() {
  const client = useQueryClient()
  const fieldId = useId()
  const [name, setName] = useState("")
  const [ssh, setSsh] = useState("")
  const [remote, setRemote] = useState("")
  const [release, setRelease] = useState(false)
  const [cli, setCli] = useState("")
  const [daemon, setDaemon] = useState("")
  const [repository, setRepository] = useState("")
  const [tag, setTag] = useState("")
  const [active, setActive] = useState("")
  const status = useQuery({
    queryKey: qk.hosts.setup(active),
    enabled: !!active,
    queryFn: () =>
      unwrap(api(LOCAL_HOST).GET("/v1/hosts/{name}/setup", { params: { path: { name: active } } })),
    refetchInterval: (query) =>
      ["pending", "running"].includes(query.state.data?.state ?? "") ? 1000 : false,
  })
  useEffect(() => {
    if (status.data?.state === "complete") {
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    }
  }, [client, status.data?.state])
  const start = useMutation({
    mutationFn: () =>
      unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup", {
          params: { path: { name } },
          body: {
            ssh,
            remote: remote || null,
            source: release ? { kind: "release", repository, tag } : { kind: "local", cli, daemon },
          },
        }),
      ),
    onSuccess: (progress) => {
      client.setQueryData(qk.hosts.setup(name), progress)
      setActive(name)
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    },
  })
  const busy = start.isPending || ["pending", "running"].includes(status.data?.state ?? "")
  return (
    <fieldset className="flex flex-col gap-2" aria-label="SSH worker setup">
      <h3>Install an SSH worker</h3>
      <p className="text-sm text-muted-foreground">
        Use SSH keys, your SSH agent, or SSH configuration. Local binary paths refer to the settings
        daemon's machine.
      </p>
      <label htmlFor={`${fieldId}-0`}>
        Worker name
        <Input
          id={`${fieldId}-0`}
          aria-label="Worker name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          pattern="[A-Za-z0-9_-]+"
          disabled={busy}
        />
      </label>
      <label htmlFor={`${fieldId}-1`}>
        SSH address
        <Input
          id={`${fieldId}-1`}
          aria-label="SSH address"
          value={ssh}
          onChange={(e) => setSsh(e.target.value)}
          placeholder="ssh://user@machine:22"
          disabled={busy}
        />
      </label>
      <label htmlFor={`${fieldId}-2`}>
        Remote socket (optional)
        <Input
          id={`${fieldId}-2`}
          value={remote}
          onChange={(e) => setRemote(e.target.value)}
          disabled={busy}
        />
      </label>
      <label>
        <input
          type="checkbox"
          checked={release}
          onChange={(e) => setRelease(e.target.checked)}
          disabled={busy}
        />
        Use a verified release
      </label>
      {release ? (
        <>
          <label htmlFor={`${fieldId}-3`}>
            Release repository
            <Input
              id={`${fieldId}-3`}
              value={repository}
              onChange={(e) => setRepository(e.target.value)}
              disabled={busy}
              placeholder="owner/repository"
            />
          </label>
          <label htmlFor={`${fieldId}-4`}>
            Release tag
            <Input
              id={`${fieldId}-4`}
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              disabled={busy}
              placeholder="v1.0.0"
            />
          </label>
        </>
      ) : (
        <>
          <label htmlFor={`${fieldId}-5`}>
            CLI binary path
            <Input
              id={`${fieldId}-5`}
              aria-label="CLI binary path"
              value={cli}
              onChange={(e) => setCli(e.target.value)}
              disabled={busy}
            />
          </label>
          <label htmlFor={`${fieldId}-6`}>
            Daemon binary path
            <Input
              id={`${fieldId}-6`}
              aria-label="Daemon binary path"
              value={daemon}
              onChange={(e) => setDaemon(e.target.value)}
              disabled={busy}
            />
          </label>
        </>
      )}
      <div className="flex gap-2">
        <Button
          type="button"
          onClick={() => start.mutate()}
          disabled={busy || !name || !ssh || (release ? !repository || !tag : !cli || !daemon)}
        >
          Start or retry setup
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!name || busy}
          onClick={() => {
            setActive(name)
            void client.invalidateQueries({ queryKey: qk.hosts.setup(name) })
          }}
        >
          Load setup progress
        </Button>
      </div>
      {start.error || status.error ? (
        <p role="alert">{describeError(start.error ?? status.error)}</p>
      ) : null}
      {status.data ? (
        <div role="status">
          <p>{status.data.state}</p>
          <ol>
            {status.data.steps.map((step) => (
              <li key={step.name}>
                <span>
                  {step.name}: {step.complete ? "complete" : "pending"}
                </span>
                {step.error ? <p role="alert">{step.error}</p> : null}
              </li>
            ))}
          </ol>
        </div>
      ) : null}
    </fieldset>
  )
}
