#!/usr/bin/env bash
# Run on any OS: bash scripts/tests/release-please-config.sh. It builds notes
# from representative Conventional Commit messages and the release config.
set -Eeuo pipefail

REPO_DIR="$(cd "$(dirname "$0")/../.." && pwd)"

node - "$REPO_DIR/release-please-config.json" <<'NODE'
const { readFileSync } = require("node:fs")

const configPath = process.argv[2]
const config = JSON.parse(readFileSync(configPath, "utf8"))
const sections = new Map(
  config["changelog-sections"].map(({ type, section, hidden }) => [
    type,
    { section, hidden },
  ]),
)
const messages = [
  "perf: reduce release-note generation time",
  "ci: run the release verification",
]

function parseCommit(message) {
  const match = message.match(/^([a-z]+)(?:\([^)]*\))?!?: (.+)$/)
  if (!match) {
    throw new Error(`cannot parse ${message}`)
  }
  return { type: match[1], subject: match[2] }
}

const notes = messages
  .map(parseCommit)
  .map(({ type, subject }) => {
    const entry = sections.get(type)
    if (!entry) {
      throw new Error(`no changelog section for ${type}`)
    }
    if (entry.hidden) {
      return ""
    }
    return `### ${entry.section}\n\n* ${subject}`
  })
  .filter(Boolean)
  .join("\n\n")

const expected = "### ⚡ Performance\n\n* reduce release-note generation time"
if (notes !== expected) {
  throw new Error(`unexpected release notes:\n${notes}`)
}
NODE

printf 'PASS: release-please-config\n'
