/**
 * Facts the AI permission model's decision carries, in the one shape the test
 * endpoint's response and a learned row's `output` both use (022, rules 27
 * and 28): a label, a danger, both thresholds, the allow and deny
 * probabilities, the cap that held, and the risk tags. Shared so the test
 * dialog, the Learned tab's row and its detail panel read the same fields the
 * same way.
 */

export type AiLabel = "allow" | "ask" | "deny"

export const AI_LABEL_TEXT: Record<AiLabel, string> = { allow: "Allow", ask: "Ask", deny: "Deny" }

export const AI_LABEL_TONE: Record<AiLabel, string> = {
  allow: "bg-status-done-soft text-status-done-fg",
  ask: "bg-status-warn-soft text-status-warn-fg",
  deny: "bg-status-danger-soft text-status-danger-fg",
}

const LABELS: readonly AiLabel[] = ["allow", "ask", "deny"]

/**
 * The allow and deny probabilities out of Kev's `probabilities` object (022,
 * rule 27): key `"0"` is the allow probability, and the highest key is the
 * deny probability — the same pair `ai_permission_metrics` formats on the
 * daemon (`crates/ariadne-api/src/permissions.rs`).
 */
export function probabilityPair(probabilities: unknown): { allow: number; deny: number } | null {
  if (probabilities === null || typeof probabilities !== "object") return null
  const entries = Object.entries(probabilities as Record<string, unknown>)
  if (entries.length === 0) return null
  const allow = (probabilities as Record<string, unknown>)["0"]
  const deny = (probabilities as Record<string, unknown>)[String(entries.length - 1)]
  if (typeof allow !== "number" || typeof deny !== "number") return null
  return { allow, deny }
}

interface AiOutputFacts {
  label: AiLabel | null
  danger: number | null
  allowThreshold: number | null
  denyThreshold: number | null
  allowProbability: number | null
  denyProbability: number | null
  cap: string | null
  riskTags: string[] | null
}

/** A learned row's own `output`: the model's decision, or `null` where it was never called. */
export function parseAiOutput(output: unknown): AiOutputFacts | null {
  if (output === null || typeof output !== "object") return null
  const value = output as Record<string, unknown>
  const probabilities = probabilityPair(value.probabilities)
  const label = value.label
  return {
    label:
      typeof label === "string" && (LABELS as readonly string[]).includes(label)
        ? (label as AiLabel)
        : null,
    danger: typeof value.danger === "number" ? value.danger : null,
    allowThreshold: typeof value.allow_threshold === "number" ? value.allow_threshold : null,
    denyThreshold: typeof value.deny_threshold === "number" ? value.deny_threshold : null,
    allowProbability: probabilities?.allow ?? null,
    denyProbability: probabilities?.deny ?? null,
    cap: typeof value.cap === "string" ? value.cap : null,
    riskTags: Array.isArray(value.risk_tags)
      ? value.risk_tags.filter((tag): tag is string => typeof tag === "string")
      : null,
  }
}
