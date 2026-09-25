/**
 * Everything the session views read from and write to the daemon.
 *
 * Nothing here subscribes to domain events: `session_created` and
 * `session_updated` already patch `sessions.detail` and invalidate
 * `sessions.lists` in `src/events/dispatch.ts`, so these queries go live for
 * free. The mutations still write their response into the cache, because the
 * action's result should be on screen before the event that confirms it
 * arrives — and because the event never arrives at all when the stream is
 * down.
 *
 * Only the session calls are here. A session view also reads the goal and the
 * task behind the ids it carries, and those are asked for where they are
 * owned (`goals/queries.ts`, `tasks/queries.ts`) rather than declared a second
 * time here: the key is the same either way, so a second declaration bought
 * nothing and was one `select` or one `staleTime` away from disagreeing with
 * the original.
 */

import {
  type InfiniteData,
  infiniteQueryOptions,
  queryOptions,
  useMutation,
  useQueryClient,
} from "@tanstack/react-query"

import {
  api,
  type CacheSnapshot,
  cacheRow,
  type NewSessionRequest,
  optimisticStatus,
  qk,
  type ResumeOutsideSessionRequest,
  restoreCache,
  type Seat,
  type SessionDto,
  type SessionEntryDto,
  type SessionStatus,
  unwrap,
} from "@/api"

import { isLiveStatus, sessionAttention } from "./session-display"

/** What the sessions list can be narrowed by. */
export interface SessionListFilters {
  goal?: string
  task?: string
  status?: SessionStatus
  /**
   * Applied here rather than by the daemon: `GET /v1/sessions` takes no seat.
   * The request — and so the cache entry — is the same one an unfiltered list
   * makes, and the seat is a per-observer `select` over it.
   */
  seat?: Seat
  /**
   * Only the sessions with an agent that may still produce output. Client-side
   * for the same reason the seat is, and for one more: the daemon's filter
   * takes *one* status, and being live is three of them (see
   * {@link isLiveStatus}). Set alongside `status` it would only narrow it
   * further, so the two are never used together.
   */
  live?: boolean
  /**
   * Only the sessions the daemon has raised a reason on. Client-side again,
   * and by {@link sessionAttention} rather than by a rule of its own: the
   * sessions screen and the attention strip have to agree on which agents are
   * asking for a person. The request is the same one an unfiltered list makes,
   * so this costs no round trip of its own.
   */
  attention?: boolean
}

export function sessionsQueryOptions({ seat, live, attention, ...query }: SessionListFilters = {}) {
  const narrowed = (session: SessionDto) =>
    (!seat || session.seat === seat) &&
    (!live || isLiveStatus(session.status)) &&
    (!attention || sessionAttention(session) !== null)
  return queryOptions({
    queryKey: qk.sessions.list(query),
    queryFn: () => everyAriadneSession(query),
    select:
      seat || live || attention ? (sessions: SessionDto[]) => sessions.filter(narrowed) : undefined,
  })
}

/** The most `GET /v1/sessions` hands back in one page. */
const PAGE_LIMIT = 200

/**
 * Every session Ariadne runs under these filters, ended ones too, as
 * `SessionDto` rows: the whole list, followed page by page.
 *
 * `GET /v1/sessions` pages Ariadne's sessions together with the outside
 * conversations, and leaves out what ended or went quiet over a week ago
 * unless `all` is set. Every view reading this list wants the lot — a task's
 * sessions, a goal's orchestrators, every stuck agent — so it asks for one
 * kind, sets `all`, and reads on until the last page.
 */
async function everyAriadneSession(query: Omit<SessionListFilters, "seat" | "live" | "attention">) {
  const sessions: SessionDto[] = []
  let cursor: string | undefined
  do {
    const page = await unwrap(
      api().GET("/v1/sessions", {
        params: { query: { ...query, kind: "ariadne", all: true, limit: PAGE_LIMIT, cursor } },
      }),
    )
    sessions.push(...page.sessions.map(ariadneSession))
    cursor = page.next_cursor ?? undefined
  } while (cursor)
  return sessions
}

/**
 * A listing row of an Ariadne session, as the `SessionDto` that
 * `GET /v1/sessions/{id}` answers for it — the shape the detail cache and
 * every session view read. The fields a listing row leaves optional are ones
 * an Ariadne session always has; only an outside row goes without them.
 */
function ariadneSession(entry: SessionEntryDto): SessionDto {
  return {
    id: entry.id,
    goal_id: entry.goal_id,
    task_id: entry.task_id,
    seat: entry.seat,
    task_agent_id: entry.task_agent_id,
    model: entry.model ?? "",
    effort: entry.effort,
    internal_session_id: entry.internal_session_id,
    worktree_path: entry.working_directory,
    status: entry.status ?? "exited",
    attention_reason: entry.attention_reason,
    attention_since: entry.attention_since,
    last_activity_at: entry.last_activity_at,
    usage: entry.usage ?? { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 },
    context_used: entry.context_used,
    context_size: entry.context_size,
    created_at: entry.created_at ?? "",
    ended_at: entry.ended_at,
    // A listing row's title is its work's where it has work; only a loose
    // session's is its own.
    title: entry.goal_id || entry.task_id ? null : entry.title,
  }
}

/**
 * A conversation Ariadne did not start: what the outside rows of
 * `GET /v1/sessions` carry.
 */
export interface OutsideSessionDto {
  agent_id: string
  /** The id the agent loads this conversation back by. */
  internal_session_id: string
  working_directory: string
  last_activity_at: string
  first_prompt: string
}

/** One page of the outside listing, its rows read as {@link OutsideSessionDto}. */
interface OutsideSessionPage {
  sessions: OutsideSessionDto[]
  next_cursor?: string | null
  total: number
  snapshot_at: string
}

function outsideSession(entry: SessionEntryDto): OutsideSessionDto {
  return {
    agent_id: entry.agent_id,
    internal_session_id: entry.internal_session_id ?? entry.id,
    working_directory: entry.working_directory ?? "",
    last_activity_at: entry.last_activity_at ?? "",
    first_prompt: entry.title ?? "",
  }
}

export function sessionQueryOptions(id: string) {
  return queryOptions({
    queryKey: qk.sessions.detail(id),
    queryFn: () => unwrap(api().GET("/v1/sessions/{id}", { params: { path: { id } } })),
  })
}

/** What the outside-sessions list can be narrowed by, as the daemon takes it. */
export interface OutsideSessionListFilters {
  agent?: string
  dir?: string
  /** RFC 3339, not a day: see `filters.ts`. */
  since?: string
  until?: string
  q?: string
}

/**
 * Conversations Ariadne did not start, ready for the user to resume: the
 * pages the daemon cuts from its snapshot, under one filter.
 *
 * `next_cursor` is the next page's parameter and a null one is the last page,
 * so the cursor never reaches the key — the pages of one filter are one cache
 * entry, which is what lets the table grow by a page rather than reload.
 *
 * `takeRefresh` is read at request time rather than keyed on: asking every
 * agent again is what the Refresh button does, not what the list is narrowed
 * by, and its answer belongs in the entry the filters already name. It is
 * taken, not read — the flag it answers from is spent on the first request of
 * a refetch, which is the first page.
 */
export function outsideSessionsQueryOptions(
  filters: OutsideSessionListFilters = {},
  takeRefresh: () => boolean = () => false,
) {
  return infiniteQueryOptions({
    queryKey: qk.outsideSessions.list(filters),
    queryFn: ({ pageParam }) =>
      unwrap(
        api().GET("/v1/sessions", {
          params: {
            query: {
              ...filters,
              kind: "outside",
              cursor: pageParam ?? undefined,
              refresh: takeRefresh() || undefined,
            },
          },
        }),
      ).then(
        (page): OutsideSessionPage => ({ ...page, sessions: page.sessions.map(outsideSession) }),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor ?? null,
  })
}

/**
 * Start a loose session: a new conversation with an agent, in a directory,
 * without a goal, task or seat. It comes back live, waiting for its first
 * prompt.
 */
export function useStartSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: NewSessionRequest) => unwrap(api().POST("/v1/sessions", { body })),
    onSuccess: (session) => cacheRow(queryClient, qk.sessions, session),
  })
}

/**
 * Resume an outside conversation as a live session, without a goal, task or seat.
 *
 * The session that answers holds the conversation from then on, so its
 * outside row goes from every cached page at once — the table would show the
 * one conversation twice until the outside lists came back — and those lists
 * are refetched too, since the daemon now leaves it out of their totals.
 */
export function useResumeOutsideSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body: ResumeOutsideSessionRequest) =>
      unwrap(api().POST("/v1/outside-sessions/resume", { body })),
    onSuccess: (session, { agent_id, internal_session_id }) => {
      cacheRow(queryClient, qk.sessions, session)
      queryClient.setQueriesData<InfiniteData<OutsideSessionPage>>(
        { queryKey: qk.outsideSessions.lists() },
        (data) =>
          data && {
            ...data,
            pages: data.pages.map((page) => ({
              ...page,
              sessions: page.sessions.filter(
                (outside) =>
                  outside.agent_id !== agent_id ||
                  outside.internal_session_id !== internal_session_id,
              ),
            })),
          },
      )
      void queryClient.invalidateQueries({ queryKey: qk.outsideSessions.lists() })
    },
  })
}

/**
 * Kill a session's agent process. Only meaningful while the session is live.
 *
 * Optimistic: the daemon stops the agent and marks the session `exited`,
 * which is the one status the client can know in advance, and a row that still
 * says "running" after a confirmed kill is the wrong thing to be looking at.
 * A refusal puts the previous row straight back. The session is the mutation's
 * variable rather than a binding of the hook — the sessions list kills the row
 * that was clicked — which is why this is not `useRowAction`.
 */
export function useKillSession() {
  const queryClient = useQueryClient()
  return useMutation<SessionDto, Error, string, CacheSnapshot | undefined>({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/sessions/{id}/kill", { params: { path: { id } } })),
    onMutate: (id) =>
      optimisticStatus(queryClient, qk.sessions, id, "exited" satisfies SessionStatus),
    onError: (_error, _id, snapshot) => restoreCache(queryClient, snapshot),
    onSuccess: (session) => cacheRow(queryClient, qk.sessions, session),
  })
}

/**
 * Revive an ended session. The daemon relaunches the session itself — same id,
 * same agent conversation — and answers with the refreshed
 * row, so the response's status is what says whether anything was revived.
 * `409` is the "not resumable" answer (no internal session id, still running,
 * agent cannot resume) and carries the reason in its envelope.
 */
export function useResumeSession() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      unwrap(api().POST("/v1/sessions/{id}/resume", { params: { path: { id } } })),
    onSuccess: (session) => cacheRow(queryClient, qk.sessions, session),
  })
}

/** Index a list response by id, for turning the ids on a session into names. */
export function byId<T extends { id: string }>(items: T[] | undefined): Map<string, T> {
  return new Map((items ?? []).map((item) => [item.id, item]))
}
