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
  const [binaries, setBinaries] = useState(true)
  const [tools, setTools] = useState(false)
  const [release, setRelease] = useState(false)
  const [cli, setCli] = useState("")
  const [daemon, setDaemon] = useState("")
  const [repository, setRepository] = useState("")
  const [tag, setTag] = useState("")
  const [agents, setAgents] = useState("")
  const [github, setGithub] = useState(false)
  const [active, setActive] = useState("")
  const status = useQuery({
    queryKey: qk.hosts.update(active),
    enabled: !!active,
    queryFn: () =>
      unwrap(
        api(LOCAL_HOST).GET("/v1/hosts/{name}/update", { params: { path: { name: active } } }),
      ),
    refetchInterval: (query) =>
      ["pending", "running"].includes(query.state.data?.state ?? "") ? 1000 : false,
  })
  useEffect(() => {
    if (status.data?.state === "complete") {
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    }
  }, [client, status.data?.state])
  const start = useMutation({
    mutationFn: () => {
      const source = release
        ? { kind: "release" as const, repository, tag }
        : { kind: "local" as const, cli, daemon }
      const selectedTools = { agents: agents.split(/[\s,]+/).filter(Boolean), github }
      return unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/update", {
          params: { path: { name } },
          body: {
            ...(binaries ? { source } : {}),
            ...(tools ? { tools: selectedTools } : {}),
          },
        }),
      )
    },
    onSuccess: (progress) => {
      client.setQueryData(qk.hosts.update(name), progress)
      setActive(name)
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    },
  })
  const busy = start.isPending || ["pending", "running"].includes(status.data?.state ?? "")
  return (
    <fieldset className="flex flex-col gap-2" aria-label="SSH worker setup">
      <h3>Update an SSH worker when idle</h3>
      <p>
        Wait for running tasks. Update selected programs, then restore scheduling. Retry failures
        with corrected selections when needed.
      </p>
      <p className="text-sm text-muted-foreground">
        Use SSH keys, your SSH agent, or SSH configuration. The source daemon machine runs the
        settings daemon. Local binary paths refer to files on that machine.
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
      <p>Use this worker’s saved SSH address and socket.</p>
      <label>
        <input
          type="checkbox"
          checked={binaries}
          onChange={(e) => setBinaries(e.target.checked)}
          disabled={busy}
        />{" "}
        Update Ariadne binaries
      </label>
      {binaries ? (
        <>
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
                CLI binary path on the source daemon machine
                <Input
                  id={`${fieldId}-5`}
                  aria-label="CLI binary path on the source daemon machine"
                  value={cli}
                  onChange={(e) => setCli(e.target.value)}
                  disabled={busy}
                />
              </label>
              <label htmlFor={`${fieldId}-6`}>
                Daemon binary path on the source daemon machine
                <Input
                  id={`${fieldId}-6`}
                  aria-label="Daemon binary path on the source daemon machine"
                  value={daemon}
                  onChange={(e) => setDaemon(e.target.value)}
                  disabled={busy}
                />
              </label>
            </>
          )}
        </>
      ) : null}
      <label>
        <input
          type="checkbox"
          checked={tools}
          onChange={(e) => setTools(e.target.checked)}
          disabled={busy}
        />{" "}
        Update agent tools
      </label>
      {tools ? (
        <>
          <label htmlFor={`${fieldId}-agents`}>
            Required agent IDs
            <Input
              id={`${fieldId}-agents`}
              value={agents}
              onChange={(e) => setAgents(e.target.value)}
              disabled={busy}
              placeholder="codex-acp, claude-acp"
            />
          </label>
          <p className="text-sm text-muted-foreground">
            Select registry IDs separated by commas. The operation installs or updates Git and
            supported packages in user-owned locations. Custom commands require manual installation.
            Missing authentication requires a separate worker login.
          </p>
          <label>
            <input
              type="checkbox"
              checked={github}
              onChange={(e) => setGithub(e.target.checked)}
              disabled={busy}
            />{" "}
            Install GitHub CLI
          </label>
        </>
      ) : null}
      <div className="flex gap-2">
        <Button
          type="button"
          onClick={() => start.mutate()}
          disabled={
            busy ||
            !name ||
            (!binaries && !tools) ||
            (binaries && (release ? !repository || !tag : !cli || !daemon))
          }
        >
          Start or retry update
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!name || busy}
          onClick={() => {
            setActive(name)
            void client.invalidateQueries({ queryKey: qk.hosts.update(name) })
          }}
        >
          Load update progress
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
