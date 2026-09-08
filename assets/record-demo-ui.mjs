#!/usr/bin/env node

/**
 * Record the desktop UI README demo.
 *
 * Run this script from any directory:
 *
 *   node assets/record-demo-ui.mjs
 *
 * It builds the release binaries, creates an isolated Ariadne home, and
 * starts its daemon with a two-second inert `cli_bin`. The scheduler can
 * create metadata rows for the demo, but it cannot start an agent process.
 * The temporary home, repository, daemon, dev server, and Playwright install
 * are removed when the recording ends.
 */

import { execFile, spawn } from "node:child_process"
import { once } from "node:events"
import { chmod, mkdtemp, mkdir, rm, stat, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import net from "node:net"
import { promisify } from "node:util"

const exec = promisify(execFile)
const root = join(dirname(fileURLToPath(import.meta.url)), "..")
const output = join(root, "assets", "demo-ui.gif")
const playwrightVersion = "1.62.1"
const viewport = { width: 1280, height: 800 }

let daemon
let devServer
let browser
let context
let scratch

async function stop(child) {
  if (!child || child.exitCode !== null || child.killed) return
  const exited = once(child, "exit").catch(() => {})
  child.kill("SIGTERM")
  await exited
}

async function cleanup() {
  if (browser) await browser.close().catch(() => {})
  if (context) await context.close().catch(() => {})
  await stop(devServer)
  await stop(daemon)
  if (scratch) await rm(scratch, { recursive: true, force: true })
}

process.on("SIGINT", () => void cleanup().finally(() => process.exit(130)))
process.on("SIGTERM", () => void cleanup().finally(() => process.exit(143)))

async function run(command, args, options = {}) {
  const child = spawn(command, args, { cwd: root, stdio: "inherit", ...options })
  const [code, signal] = await once(child, "exit")
  if (code !== 0) throw new Error(`${command} ${args.join(" ")} ended with ${signal ?? `code ${code}`}`)
}

async function freePort() {
  const server = net.createServer()
  server.listen(0, "127.0.0.1")
  await once(server, "listening")
  const address = server.address()
  if (!address || typeof address === "string") throw new Error("cannot reserve a loopback port")
  await new Promise((resolve) => server.close(resolve))
  return address.port
}

async function waitFor(check, description) {
  const deadline = Date.now() + 20_000
  while (Date.now() < deadline) {
    try {
      const value = await check()
      if (value) return value
    } catch {
      // The process can be listening before its first request succeeds.
    }
    await new Promise((resolve) => setTimeout(resolve, 150))
  }
  throw new Error(`timed out waiting for ${description}`)
}

async function api(baseUrl, path, { method = "GET", body, session } = {}) {
  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers: {
      ...(body ? { "content-type": "application/json" } : {}),
      ...(session ? { "X-Ariadne-Session": session } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${await response.text()}`)
  return response.status === 204 ? undefined : response.json()
}

async function firstSession(baseUrl, query, seat) {
  return waitFor(async () => {
    const sessions = await api(baseUrl, `/v1/sessions?${query}`)
    return sessions.find((session) => session.seat === seat)
  }, `${seat} demo session`)
}

async function seed(baseUrl, repositoryPath) {
  const repository = await api(baseUrl, "/v1/repositories", {
    method: "POST",
    body: {
      path: repositoryPath,
      base_branch: "main",
      description: "The repository used by the README UI demo.",
    },
  })
  const goal = await api(baseUrl, "/v1/goals", {
    method: "POST",
    body: {
      title: "Launch the engineering dashboard",
      description: "Give the engineering team one clear view of delivery work and release readiness.",
      repository_ids: [repository.id],
      model: "codex:gpt-5.6-sol",
    },
  })
  const draft = await api(baseUrl, `/v1/goals/${goal.id}/tasks`, {
    method: "POST",
    body: {
      title: "Design the dashboard overview",
      description: "Show the delivery stages, the work that needs attention, and the most useful progress signals.",
      agents: [
        { seat: "author", skills: ["coding"], model: "codex:gpt-5.6-sol" },
        { seat: "reviewer", skills: ["code-review"], model: "codex:gpt-5.6-luna" },
      ],
    },
  })
  await api(baseUrl, `/v1/goals/${goal.id}/tasks`, {
    method: "POST",
    body: {
      title: "Prepare the release notes",
      description: "Summarize the visible changes after the dashboard ships.",
      depends_on: [draft.id],
      agents: [{ seat: "author", skills: ["coding"], model: "codex:gpt-5.6-luna" }],
    },
  })

  const orchestrator = await firstSession(baseUrl, `goal=${goal.id}`, "orchestrator")
  await api(baseUrl, `/v1/goals/${goal.id}/finalize`, {
    method: "POST",
    body: {},
    session: orchestrator.id,
  })
  const author = await firstSession(baseUrl, `task=${draft.id}`, "author")
  await waitFor(async () => {
    const task = await api(baseUrl, `/v1/tasks/${draft.id}`)
    return task.status === "in_progress"
  }, "the active demo task")
  await api(baseUrl, `/v1/tasks/${draft.id}/transitions`, {
    method: "POST",
    body: { to: "under_review", reason: "The overview is ready for feedback.", merge_commit: null },
    session: author.id,
  })

  const planningGoal = await api(baseUrl, "/v1/goals", {
    method: "POST",
    body: {
      title: "Improve first-run guidance",
      description: "Make the first project setup clear for a new team member.",
      repository_ids: [repository.id],
      model: "codex:gpt-5.6-luna",
    },
  })
  await api(baseUrl, `/v1/goals/${planningGoal.id}/tasks`, {
    method: "POST",
    body: {
      title: "Review the setup flow",
      description: "List the steps that a new contributor must complete before the first task.",
      agents: [{ seat: "author", skills: ["coding"], model: "codex:gpt-5.6-luna" }],
    },
  })

  return { goal, task: draft }
}

async function main() {
  await run("cargo", ["build", "--release"])
  const { stdout } = await exec("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd: root })
  const { target_directory: targetDirectory } = JSON.parse(stdout)
  const daemonPath = join(targetDirectory, "release", "ariadned")

  scratch = await mkdtemp(join(tmpdir(), "ariadne-demo-ui-"))
  const home = join(scratch, "home")
  const repositoryPath = join(scratch, "repository")
  const playwrightRoot = join(scratch, "playwright")
  const videoDirectory = join(scratch, "video")
  const inertCli = join(scratch, "inert-cli")
  const port = await freePort()
  const baseUrl = `http://127.0.0.1:${port}`

  await mkdir(home)
  await mkdir(repositoryPath)
  await writeFile(inertCli, "#!/bin/sh\nsleep 2\n")
  await chmod(inertCli, 0o755)
  await writeFile(
    join(home, "config.toml"),
    `tcp_listen = "127.0.0.1:${port}"\ncli_bin = "${inertCli}"\nprevent_sleep = false\n`,
  )
  await run("git", ["init", "-q", "-b", "main", repositoryPath])
  await run("git", ["-C", repositoryPath, "-c", "user.name=Demo", "-c", "user.email=demo@example.invalid", "commit", "-q", "--allow-empty", "-m", "chore: seed demo repository"])

  daemon = spawn(daemonPath, [], {
    cwd: root,
    env: { ...process.env, ARIADNE_HOME: home, RUST_LOG: "warn" },
    stdio: ["ignore", "pipe", "pipe"],
  })
  daemon.stderr.on("data", (data) => process.stderr.write(data))
  await waitFor(async () => {
    const response = await fetch(`${baseUrl}/v1/goals`)
    return response.ok
  }, "the demo daemon")

  const demo = await seed(baseUrl, repositoryPath)
  devServer = spawn("npm", ["run", "dev", "--", "--host", "127.0.0.1"], {
    cwd: join(root, "ui"),
    env: process.env,
    stdio: ["ignore", "pipe", "pipe"],
  })
  devServer.stdout.on("data", (data) => process.stdout.write(data))
  devServer.stderr.on("data", (data) => process.stderr.write(data))
  await waitFor(async () => {
    const response = await fetch("http://127.0.0.1:1420")
    return response.ok
  }, "the UI dev server")

  await mkdir(playwrightRoot)
  await run("npm", ["install", "--no-audit", "--no-fund", `playwright@${playwrightVersion}`], {
    cwd: playwrightRoot,
  })
  await run(join(playwrightRoot, "node_modules", ".bin", "playwright"), ["install", "chromium"], {
    cwd: playwrightRoot,
    stdio: "ignore",
  })
  const { chromium } = await import(
    pathToFileURL(join(playwrightRoot, "node_modules", "playwright", "index.mjs")).href
  )
  browser = await chromium.launch()
  context = await browser.newContext({
    colorScheme: "dark",
    viewport,
    recordVideo: { dir: videoDirectory, size: viewport },
  })
  await context.addInitScript((url) => {
    localStorage.setItem("ariadne.theme", "dark")
    localStorage.setItem(
      "ariadne.settings",
      JSON.stringify({ state: { baseUrl: url, goalStatusFilter: "planning,active" }, version: 0 }),
    )
  }, baseUrl)
  const page = await context.newPage()
  await page.goto("http://127.0.0.1:1420/#/goals")
  await page.getByRole("heading", { name: "Goals" }).waitFor()
  await page.getByRole("link", { name: demo.goal.title, exact: true }).waitFor()
  await page.waitForTimeout(1_200)

  await page.getByRole("link", { name: demo.goal.title, exact: true }).click()
  await page.getByRole("heading", { name: demo.goal.title, exact: true }).waitFor()
  await page.waitForTimeout(1_200)

  await page.getByRole("link", { name: demo.task.title }).click()
  await page.getByRole("heading", { name: demo.task.title, exact: true }).waitFor()
  await page.waitForTimeout(1_200)

  await page.keyboard.press("Escape")
  await page.waitForTimeout(400)
  await page.keyboard.press("Escape")
  await page.getByRole("button", { name: /Search/ }).click()
  await page.getByRole("dialog", { name: "Command palette" }).waitFor()
  await page.getByPlaceholder("Search goals, tasks, sessions, skills…").fill("dashboard")
  await page.waitForTimeout(1_600)

  const video = page.video()
  if (!video) throw new Error("Playwright did not start video recording")
  await page.close()
  await context.close()
  context = undefined
  await browser.close()
  browser = undefined
  const videoPath = await video.path()
  await run("ffmpeg", [
    "-y",
    "-i",
    videoPath,
    "-filter_complex",
    "trim=start=0.8,setpts=PTS-STARTPTS,fps=8,scale=1120:-2:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128:stats_mode=diff[p];[s1][p]paletteuse=dither=sierra2_4a",
    "-loop",
    "0",
    output,
  ])
  const size = (await stat(output)).size
  if (size >= 10 * 1024 * 1024) throw new Error(`demo-ui.gif is ${(size / 1024 / 1024).toFixed(1)} MB`)
  console.log(`Wrote ${output} (${(size / 1024 / 1024).toFixed(1)} MB)`)
}

try {
  await main()
} finally {
  await cleanup()
}
