/**
 * Wraps `npm audit --audit-level=high`, the same bar `ui/scripts/check-audit.mjs`
 * holds its own tree to, and for the same reason: a fresh high or critical
 * finding still fails exactly as the bare command would, and `ALLOWLIST` is
 * this package's own copy of that file's per-advisory ignore list, not a
 * shared one — the two trees carry different findings.
 *
 * `decide` is the seam a test drives directly, against a parsed report, with
 * no subprocess and no live advisory data.
 */
import { execFileSync } from "node:child_process"

export const ALLOWLIST = [
  {
    id: "GHSA-vfj7-8cjw-p6xm",
    package: "braces",
    reason:
      "a stack-exhaustion denial of service reached through patch-package's own find-yarn-workspace-root/micromatch chain — patch-package runs only as this package's postinstall, never in the shipped build, and every published braces release is vulnerable, so there is no version to pin an override to",
    added: "2026-10-18",
  },
]

const ALLOWED_IDS = new Set(ALLOWLIST.map((entry) => entry.id))

export function decide(report) {
  if (typeof report.vulnerabilities !== "object" || report.vulnerabilities === null) return false

  const vulnerable = Object.values(report.vulnerabilities).filter(
    (entry) => entry.severity === "high" || entry.severity === "critical",
  )
  if (vulnerable.length === 0) return true

  const advisories = new Set()
  for (const entry of vulnerable) {
    for (const via of entry.via) {
      if (typeof via === "object" && via.url) advisories.add(via.url.split("/").pop())
    }
  }

  return advisories.size > 0 && [...advisories].every((id) => ALLOWED_IDS.has(id))
}

function runAudit(args) {
  try {
    return { exitCode: 0, stdout: execFileSync("npm", args, { encoding: "utf8" }) }
  } catch (error) {
    return {
      exitCode: typeof error.status === "number" ? error.status : 1,
      stdout: error.stdout ?? "",
    }
  }
}

function main() {
  const { exitCode, stdout } = runAudit(["audit", "--audit-level=high", "--json"])
  if (exitCode === 0) process.exit(0)

  try {
    if (decide(JSON.parse(stdout))) {
      const ids = ALLOWLIST.map((entry) => entry.id).join(", ")
      console.log(`npm audit: only the allowlisted ${ids} (ui/AGENTS.md) is outstanding; passing.`)
      process.exit(0)
    }
  } catch {
    // Not JSON `npm audit` would recognize as its own report: fall through
    // and let the plain command's own output and exit code explain why.
  }

  execFileSync("npm", ["audit", "--audit-level=high"], { stdio: "inherit" })
  process.exit(exitCode)
}

if (import.meta.url === `file://${process.argv[1]}`) main()
