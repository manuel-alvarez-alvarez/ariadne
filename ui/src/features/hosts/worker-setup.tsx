import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useId, useState } from "react"
import { api, LOCAL_HOST, unwrap } from "@/api/client"
import { qk } from "@/api/query-keys"
import type { components } from "@/api/schema"
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
  const [binaries, setBinaries] = useState(true)
  const [tools, setTools] = useState(false)
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
  const setupRequest = {
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
    source: release
      ? { kind: "release" as const, repository, tag }
      : { kind: "local" as const, cli, daemon },
  }
  const fingerprint = JSON.stringify({ name, ...setupRequest })
  const [inspection, setInspection] = useState<{
    connection: string
    data: components["schemas"]["InspectionDto"]
  } | null>(null)
  const [review, setReview] = useState<{
    fingerprint: string
    data: components["schemas"]["SetupReviewDto"]
  } | null>(null)
  const connection = JSON.stringify({ name, ssh })
  const inspected = inspection?.connection === connection
  const reviewed = inspected && review?.fingerprint === fingerprint
  const inspect = useMutation({
    mutationFn: async () => ({
      connection,
      data: await unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/inspect", {
          params: { path: { name } },
          body: { ssh },
        }),
      ),
    }),
    onSuccess: setInspection,
  })
  const preview = useMutation({
    mutationFn: async () => ({
      fingerprint,
      data: await unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/review", {
          params: { path: { name } },
          body: setupRequest,
        }),
      ),
    }),
    onSuccess: setReview,
  })
  const start = useMutation({
    mutationFn: () => {
      const source = release
        ? { kind: "release" as const, repository, tag }
        : { kind: "local" as const, cli, daemon }
      const selectedTools = { agents: agents.split(/[\s,]+/).filter(Boolean), github }
      if (update) {
        return unwrap(
          api(LOCAL_HOST).POST("/v1/hosts/{name}/update", {
            params: { path: { name } },
            body: {
              ...(binaries ? { source } : {}),
              ...(tools ? { tools: selectedTools } : {}),
            },
          }),
        )
      }
      return unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup", {
          params: { path: { name } },
          body: setupRequest,
        }),
      )
    },
    onSuccess: (progress) => {
      client.setQueryData(progressKey(name), progress)
      setActive(name)
      void client.invalidateQueries({ queryKey: qk.hosts.lists() })
    },
  })
  const resume = useMutation({
    mutationFn: () =>
      unwrap(
        api(LOCAL_HOST).POST("/v1/hosts/{name}/setup/resume", {
          params: { path: { name: active } },
        }),
      ),
    onSuccess: (progress) => {
      client.setQueryData(progressKey(active), progress)
    },
  })
  const busy =
    resume.isPending ||
    inspect.isPending ||
    preview.isPending ||
    start.isPending ||
    ["pending", "running"].includes(status.data?.state ?? "")
  return (
    <fieldset className="flex flex-col gap-2" aria-label="SSH worker setup">
      <h3>{update ? "Update an SSH worker when idle" : "Install an SSH worker"}</h3>
      {update ? (
        <p>
          Wait for running tasks. Update selected programs, then restore scheduling. Retry failures
          with corrected selections when needed.
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
      {update ? (
        <p>Use this worker’s saved SSH address and socket.</p>
      ) : (
        <>
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
        </>
      )}
      {update ? (
        <label>
          <input
            type="checkbox"
            checked={binaries}
            onChange={(e) => setBinaries(e.target.checked)}
            disabled={busy}
          />{" "}
          Update Ariadne binaries
        </label>
      ) : null}
      {!update || binaries ? (
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
      {update ? (
        <label>
          <input
            type="checkbox"
            checked={tools}
            onChange={(e) => setTools(e.target.checked)}
            disabled={busy}
          />{" "}
          Update agent tools
        </label>
      ) : null}
      {!update || tools ? (
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
      {!update ? (
        <>
          <Button type="button" disabled={busy || !name || !ssh} onClick={() => inspect.mutate()}>
            Inspect worker
          </Button>
          {inspected && inspection ? (
            <div role="status">
              <p>Worker target: {inspection.data.target}</p>
              <p>Supported agents: {inspection.data.supported_agents.join(", ")}</p>
              {inspection.data.checks.map((check) => (
                <p key={check.id}>
                  {check.id}: {check.satisfied ? "available" : check.missing}{" "}
                  {!check.satisfied ? `${check.resolution} ${check.recheck}` : ""}
                </p>
              ))}
              <Button
                type="button"
                disabled={
                  busy ||
                  (!cli && !release) ||
                  (!daemon && !release) ||
                  (release && (!repository || !tag))
                }
                onClick={() => preview.mutate()}
              >
                Review setup
              </Button>
            </div>
          ) : null}
          {reviewed && review ? (
            <section aria-label="Setup preview">
              <h4>Planned actions</h4>
              <ul>
                {review.data.actions.map((action) => (
                  <li key={action}>{action}</li>
                ))}
              </ul>
              <h4>Preserved settings</h4>
              <ul>
                {review.data.preserved.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
              <h4>Prerequisites</h4>
              {review.data.prerequisites.map((check) => (
                <p key={check.id}>
                  {check.id}:{" "}
                  {check.satisfied
                    ? "satisfied"
                    : `${check.missing} ${check.resolution} ${check.recheck}`}
                </p>
              ))}
              <p>
                Apply independent actions now. Setup stays incomplete until all required actions
                succeed.
              </p>
            </section>
          ) : null}
        </>
      ) : null}
      <div className="flex gap-2">
        <Button
          type="button"
          onClick={() => start.mutate()}
          disabled={
            busy ||
            (!update && !reviewed) ||
            !name ||
            (!update && !ssh) ||
            (update && !binaries && !tools) ||
            ((!update || binaries) && (release ? !repository || !tag : !cli || !daemon))
          }
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
      {resume.error || inspect.error || preview.error || start.error || status.error ? (
        <p role="alert">
          {describeError(
            resume.error ?? inspect.error ?? preview.error ?? start.error ?? status.error,
          )}
        </p>
      ) : null}
      {status.data ? (
        <div role="status">
          <p>{status.data.state}</p>
          {!update && ["incomplete", "failed"].includes(status.data.state) ? (
            <Button type="button" disabled={busy} onClick={() => resume.mutate()}>
              Recheck and resume setup
            </Button>
          ) : null}
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
