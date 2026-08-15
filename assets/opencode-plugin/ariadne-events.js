/**
 * Ariadne events plugin for OpenCode.
 *
 * Forwards session/tool events to the Ariadne daemon via the fail-safe
 * `ariadne agent-event` subcommand. No-ops entirely unless the process was
 * spawned by Ariadne (ARIADNE_SESSION_ID present), so it is safe to keep
 * installed globally.
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

  const interesting = new Set([
    "session.created",
    "session.updated",
    "session.idle",
    "session.error",
    "session.deleted",
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
