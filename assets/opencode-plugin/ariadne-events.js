/**
 * Ariadne events plugin for OpenCode.
 *
 * Forwards session/tool/approval events to the Ariadne daemon via the
 * fail-safe `ariadne agent-event` subcommand, and tags the events that end a
 * turn — and, at most every `usageIntervalMs`, the tool calls in between —
 * with the session's token usage. No-ops entirely unless the process was
 * spawned by Ariadne (ARIADNE_SESSION_ID present), so it is safe to keep
 * installed globally.
 */
export const AriadneEvents = async ({ $, client }) => {
  const sessionId = process.env["ARIADNE_SESSION_ID"];
  if (!sessionId) return {};

  const cli = process.env["ARIADNE_CLI"] || "ariadne";

  const forward = async (kind, payload) => {
    try {
      const body = JSON.stringify({ kind, payload });
      await $`${cli} agent-event --kind opencode --json ${body}`.quiet().nothrow();
    } catch {
      // Never let event forwarding break the agent.
    }
  };

  // Token usage, attached to the events that end a turn and to the tool calls
  // in between (a turn is one long stretch of them, and its figures would
  // otherwise sit at zero until it ends). Verified against
  // opencode 1.18.15 and @opencode-ai/sdk 1.18.10:
  //
  //   session.idle          {sessionID}
  //   session.error         {sessionID?, error?}
  //   client.session.get      -> {data: {id, parentID?, ...}}
  //   client.session.children -> {data: [session, ...]}
  //   client.session.messages -> {data: [{info, parts}, ...]}, where an
  //                              assistant `info` carries
  //                              tokens: {input, output, reasoning,
  //                                       cache: {read, write}}
  //
  // Subagents (the `task` tool) run in a session of their own whose
  // `parentID` is this one — every `(@… subagent)` row in
  // ~/.local/share/opencode/opencode.db has a `parent_id`, and none of them
  // nests any deeper. Their tokens are this session's consumption, so the
  // root rolls its children in and a child's own idle reports nothing:
  // Ariadne stores cumulative totals per source, and reporting both would
  // count the children twice.
  const usageDeadlineMs = 2000;

  // Walking every message of the session and its children after every tool
  // call would be dozens of walks a turn for an answer that moves slowly, so
  // a tool call looks the totals up only when the last lookup is this old.
  // The events that end a turn always look them up, and every lookup —
  // whatever it comes back with — restarts the interval. The plugin outlives
  // the turn, so the timestamp is just a variable here.
  const usageIntervalMs = 10000;
  let lastUsageAt = -Infinity;
  const usageIsDue = () => Date.now() - lastUsageAt >= usageIntervalMs;

  // Every reader below throws on anything it does not recognise, and the
  // caller turns that into no usage at all. A shape this code cannot read is
  // a number it cannot trust, and a plausible zero is worse than a gap.
  const fail = (what, value) => {
    throw new Error(`opencode sdk: ${what}: ${JSON.stringify(value)}`);
  };
  // The SDK resolves failures into `{error}` rather than throwing.
  const body = (result) => {
    if (result?.error) fail("request failed", result.error);
    return result?.data ?? result;
  };
  const record = (value, what) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${what} is not an object`, value);
    return value;
  };
  const list = (result, what) => {
    const data = body(result);
    if (!Array.isArray(data)) fail(`${what} is not a list`, data);
    return data;
  };
  const count = (value, what) => {
    if (!Number.isFinite(value)) fail(`${what} is not a number`, value);
    return value;
  };

  // What a message's `tokens` record means, read out of opencode 1.18.15's own
  // build on 2026-08-28 — its five counters are disjoint, so the contract in
  // `crates/ariadne-api/src/usage.rs` is reached by adding, never by trusting
  // one of them to contain another:
  //
  //   input     the prompt minus both cache figures (`nonCachedInputTokens`,
  //             built as `inputTokens - cacheRead - cacheWrite`)
  //   cache.read / cache.write   the two cache figures, side by side with it
  //   output    the completion minus reasoning (`visibleOutputTokens`)
  //   reasoning the reasoning, side by side with it
  //
  // opencode adds them back up the same way: its own per-session total is
  // `input + output + reasoning + cache.read + cache.write`, and its per-model
  // stats sum `output + reasoning` into one output column, as below.
  const sumUsage = async (id) => {
    const session = record(body(await client.session.get({ path: { id } })), "session");
    if (session.parentID) return undefined;

    const sessions = [id];
    for (const child of list(await client.session.children({ path: { id } }), "children")) {
      const childID = record(child, "child session").id;
      if (typeof childID !== "string") fail("child session has no id", child);
      sessions.push(childID);
    }

    const usage = { source: id, input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 };
    for (const sessionID of sessions) {
      for (const entry of list(await client.session.messages({ path: { id: sessionID } }), "messages")) {
        const { tokens } = record(record(entry, "message").info ?? entry, "message info");
        // A user message has no tokens at all; that is the only silent skip.
        if (tokens === undefined || tokens === null) continue;
        const { cache } = record(tokens, "tokens");
        record(cache, "token cache");
        usage.input_tokens +=
          count(tokens.input, "tokens.input") +
          count(cache.read, "cache.read") +
          count(cache.write, "cache.write");
        usage.cached_input_tokens += cache.read;
        usage.output_tokens += count(tokens.output, "tokens.output") + count(tokens.reasoning, "tokens.reasoning");
      }
    }
    return usage;
  };

  // Best effort throughout: a missing client, a failed request, a shape any
  // reader above rejects or a slow server all forward the event exactly as it
  // would have been forwarded without usage, and none of them delays idle by
  // more than the deadline.
  const usageOf = async (id) => {
    if (!client || !id) return undefined;
    lastUsageAt = Date.now();
    let timer;
    try {
      const deadline = new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("usage lookup timed out")), usageDeadlineMs);
      });
      return await Promise.race([sumUsage(id), deadline]);
    } catch {
      return undefined;
    } finally {
      clearTimeout(timer);
    }
  };

  // Both of these end a turn, so both carry the totals so far.
  const endsTurn = new Set(["session.idle", "session.error"]);

  // Lifecycle, plus the two families where OpenCode stops and waits for a
  // human. Verified against opencode 1.18.15 by logging every event the
  // plugin `event` hook receives during a real run:
  //
  //   permission.asked    {id, sessionID, permission, patterns, metadata,
  //                        always, tool: {messageID, callID}}
  //   permission.replied  {sessionID, requestID, reply}
  //
  // `permission.updated` is what the generated SDK types
  // (@opencode-ai/sdk 1.18.10) still call the ask; the 1.18.15 runtime never
  // emits it, and it costs nothing to keep an older opencode visible too.
  //
  // The `question.*` family is the same shape for the `question` tool, which
  // asks the user rather than the approval layer. Ariadne's own config denies
  // that tool (see the opencode adapter), so these are the safety net for a
  // session whose permissions someone changed after attaching — not the
  // common path, and the only ones here not observed on the wire.
  //
  // The `permission.ask` plugin hook is deliberately not used: 1.18.15 never
  // calls it, the event bus is where approvals surface now.
  const interesting = new Set([
    "session.created",
    "session.updated",
    "session.idle",
    "session.error",
    "session.deleted",
    "permission.asked",
    "permission.updated",
    "permission.replied",
    "question.asked",
    "question.replied",
    "question.rejected",
  ]);

  return {
    event: async ({ event }) => {
      if (!event || !event.type) return;
      if (!interesting.has(event.type) && !event.type.startsWith("tool.")) return;
      const payload = event.properties ?? {};
      if (endsTurn.has(event.type)) {
        const usage = await usageOf(payload.sessionID);
        if (usage) {
          await forward(event.type, { ...payload, ariadne_usage: usage });
          return;
        }
      }
      await forward(event.type, payload);
    },
    "tool.execute.after": async (input, output) => {
      const payload = {
        tool: input?.tool,
        sessionID: input?.sessionID,
        title: output?.title,
      };
      const usage = usageIsDue() ? await usageOf(input?.sessionID) : undefined;
      await forward("tool.execute.after", usage ? { ...payload, ariadne_usage: usage } : payload);
    },
  };
};
