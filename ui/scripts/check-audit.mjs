/**
 * Wraps `npm audit --audit-level=high`: a fresh finding at or above high
 * still fails exactly as the bare command would. The one documented,
 * currently unfixable advisory (ui/AGENTS.md, "Dependency advisories") does
 * not: every release of `braces` up to its own latest is vulnerable, so
 * there is no version to pin an override to, and `npm audit` itself has no
 * per-advisory ignore list the way `.cargo/audit.toml` does for Rust.
 *
 * `decide` is the seam a test drives directly, against a parsed report, with
 * no subprocess and no live advisory data.
 */
import { execFileSync } from "node:child_process"

export const ALLOWED_ADVISORY = "GHSA-vfj7-8cjw-p6xm"
export const ALLOWED_PACKAGES = new Set([
  "braces",
  "fast-glob",
  "micromatch",
  "@ts-morph/common",
  "ts-morph",
  "shadcn",
])

export function decide(report) {
  const vulnerable = Object.values(report.vulnerabilities ?? {}).filter(
    (entry) => entry.severity === "high" || entry.severity === "critical",
  )
  if (vulnerable.length === 0) return true

  const advisories = new Set()
  for (const entry of vulnerable) {
    for (const via of entry.via) {
      if (typeof via === "object" && via.url) advisories.add(via.url.split("/").pop())
    }
  }

  return (
    vulnerable.every((entry) => ALLOWED_PACKAGES.has(entry.name)) &&
    [...advisories].every((id) => id === ALLOWED_ADVISORY) &&
    advisories.has(ALLOWED_ADVISORY)
  )
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
      console.log(
        `npm audit: only the documented ${ALLOWED_ADVISORY} (ui/AGENTS.md) is outstanding; passing.`,
      )
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
