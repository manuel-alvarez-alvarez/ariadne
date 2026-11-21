import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useId, useState } from "react"
import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { describeError } from "@/lib/format"

export function WorkerSetup({ update = false }: { update?: boolean }) {
  const operation = update ? "update" : "setup"
  const route = update ? "/v1/hosts/{name}/update" : "/v1/hosts/{name}/setup"
  const progressKey = update ? qk.hosts.update : qk.hosts.setup
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
  const [agents, setAgents] = useState("")
  const [github, setGithub] = useState(false)
  const [codexCredentials, setCodexCredentials] = useState(false)
  const [githubCredentials, setGithubCredentials] = useState(false)
  const [active, setActive] = useState("")
  const [destinations, setDestinations] = useState<Record<string, string>>({})
  const repositories = useQuery({
    queryKey: qk.repositories.list(LOCAL_HOST),
    queryFn: () => unwrap(api(LOCAL_HOST).GET("/v1/repositories")),
  })
  const status = useQuery({
    queryKey: progressKey(active),
    enabled: !!active,
    queryFn: () => unwrap(api(LOCAL_HOST).GET(route, { params: { path: { name: active } } })),
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
        api(LOCAL_HOST).POST(route, {
          params: { path: { name } },
          body: {
            ...(Object.keys(destinations).length ? { repositories: destinations } : {}),
            ...(agents.trim() || github
              ? { tools: { agents: agents.split(/[\s,]+/).filter(Boolean), github } }
              : {}),
            ...(codexCredentials || githubCredentials
              ? {
                  credentials: [
                    ...(codexCredentials ? ["codex"] : []),
                    ...(githubCredentials ? ["github"] : []),
                  ],
                }
              : {}),
            ssh,
            remote: remote || null,
            source: release ? { kind: "release", repository, tag } : { kind: "local", cli, daemon },
          },
        }),
      ),
    onSuccess: (progress) => {
      client.setQueryData(progressKey(name), progress)
      setActive(name)
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    },
  })
  const busy = start.isPending || ["pending", "running"].includes(status.data?.state ?? "")
  return (
    <fieldset className="flex flex-col gap-2" aria-label="SSH worker setup">
      <h3>{update ? "Update an SSH worker when idle" : "Install an SSH worker"}</h3>
      {update ? (
        <p>
          Wait for running tasks. Update selected programs, then restore scheduling. Retry failures
          with the same request.
        </p>
      ) : null}
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
        Select registry IDs separated by commas. The operation installs or updates Git and supported
        packages in user-owned locations. Custom commands require manual installation. Missing
        authentication requires a separate worker login.
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
      {update ? null : (
        <>
          <label>
            <input
              type="checkbox"
              checked={codexCredentials}
              onChange={(e) => setCodexCredentials(e.target.checked)}
              disabled={busy}
            />{" "}
            Copy Codex file credentials
          </label>
          <label>
            <input
              type="checkbox"
              checked={githubCredentials}
              onChange={(e) => setGithubCredentials(e.target.checked)}
              disabled={busy}
            />{" "}
            Copy GitHub credentials
          </label>
          <p className="text-sm text-muted-foreground">
            Copy only selected credentials from the source daemon machine through SSH. Keychain
            credentials require separate worker login.
          </p>
          <fieldset disabled={busy}>
            <legend>Worker repositories</legend>
            <p>Clone selected Git remotes under ~/workspace. Change each destination as needed.</p>
            {repositories.data?.map((repo) => (
              <div key={repo.id}>
                <label>
                  <input
                    type="checkbox"
                    checked={repo.id in destinations}
                    onChange={(event) => {
                      setDestinations((previous) => {
                        const next = { ...previous }
                        if (event.target.checked)
                          next[repo.id] =
                            `~/workspace/${repo.forge?.name ?? repo.path.split("/").pop()}`
                        else delete next[repo.id]
                        return next
                      })
                    }}
                  />{" "}
                  {repo.path}
                </label>
                {repo.id in destinations ? (
                  <Input
                    aria-label={`Destination for ${repo.path}`}
                    value={destinations[repo.id]}
                    onChange={(event) =>
                      setDestinations((previous) => ({
                        ...previous,
                        [repo.id]: event.target.value,
                      }))
                    }
                  />
                ) : null}
              </div>
            ))}
            {repositories.error ? <p role="alert">{describeError(repositories.error)}</p> : null}
          </fieldset>
        </>
      )}
      <div className="flex gap-2">
        <Button
          type="button"
          onClick={() => start.mutate()}
          disabled={busy || !name || !ssh || (release ? !repository || !tag : !cli || !daemon)}
        >
          Start or retry {operation}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={!name || busy}
          onClick={() => {
            setActive(name)
            void client.invalidateQueries({ queryKey: progressKey(name) })
          }}
        >
          Load {operation} progress
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
