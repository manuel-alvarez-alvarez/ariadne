/**
 * Ariadne events plugin for OpenCode.
 *
 * Forwards session/tool/approval events to the Ariadne daemon via the
 * fail-safe `ariadne agent-event` subcommand. No-ops entirely unless the
 * process was spawned by Ariadne (ARIADNE_SESSION_ID present), so it is safe
 * to keep installed globally.
 */
export const AriadneEvents = async ({ $ }) => {
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
      if (interesting.has(event.type) || event.type.startsWith("tool.")) {
        await forward(event.type, event.properties ?? {});
      }
    },
    "tool.execute.after": async (input, output) => {
      await forward("tool.execute.after", {
        tool: input?.tool,
        sessionID: input?.sessionID,
        title: output?.title,
      });
    },
  };
};
