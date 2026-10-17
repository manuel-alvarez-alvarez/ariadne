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
 * It seeds the scratch repository with the same small login HTTP service
 * assets/demo.tape seeds (assets/seed-demo-repo.sh), fills the New goal
 * form in the UI with the same goal assets/demo.tape types, then seeds the
 * same two tasks over the REST API assets/seed-demo-goal.py also seeds them
 * with, so both recordings show the same goal and the same tasks. The
 * temporary home, repository, daemon, dev server, and Playwright install are
 * removed when the recording ends.
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

// Kept in step with assets/demo.tape and assets/seed-demo-goal.py: the same
// goal, by the same title, is what that script finds and adds tasks to.
const GOAL_TITLE = "Add rate limiting to the login endpoint"
const GOAL_DESCRIPTION =
  "Limit the login endpoint to 5 attempts per minute per IP address. " +
  "Respond 429 Too Many Requests, with a Retry-After header, once a caller goes over the limit."
const MODEL = "codex-acp:gpt-5.6-sol"

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

async function api(baseUrl, path, { method = "GET", body } = {}) {
  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!response.ok) throw new Error(`${method} ${path}: ${response.status} ${await response.text()}`)
  return response.status === 204 ? undefined : response.json()
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
  await run("bash", [join(root, "assets", "seed-demo-repo.sh"), repositoryPath])

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

  await api(baseUrl, "/v1/repositories", {
    method: "POST",
    body: {
      path: repositoryPath,
      base_branch: "main",
      description: "The login service used by the README UI demo.",
    },
  })

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
  await page.waitForTimeout(800)

  await page.getByRole("banner").getByRole("button", { name: "New goal" }).click()
  await page.getByRole("dialog", { name: "New goal" }).waitFor()
  await page.locator("#goal-title").fill(GOAL_TITLE)
  await page.locator("#goal-description").fill(GOAL_DESCRIPTION)

  await page.getByRole("combobox", { name: "Repositories", exact: true }).click()
  await page.getByRole("option", { name: new RegExp(repositoryPath.replaceAll("/", "\\/")) }).click()
  await page.getByRole("combobox", { name: "Repositories", exact: true }).click()

  await page.getByRole("button", { name: "Orchestrator runs on" }).click()
  await page.getByPlaceholder("Search models, or type an id…").fill(MODEL)
  await page.getByRole("option", { name: MODEL }).click()

  await page.getByRole("button", { name: "Create goal" }).click()
  await page.getByRole("heading", { name: GOAL_TITLE, exact: true }).waitFor()
  await page.waitForTimeout(1_200)

  await exec("python3", [join(root, "assets", "seed-demo-goal.py"), baseUrl, MODEL])

  const limiterTask = "Add the rate limiter middleware"
  // Both the attention strip (behind the panel's scrim) and the panel's own
  // task list link to the task by the same name; only the task list's own
  // row also names its column, which is what picks that one.
  const taskRow = page.getByRole("link", { name: `${limiterTask} Develop` })
  await taskRow.waitFor()
  await page.waitForTimeout(600)

  await taskRow.click()
  await page.getByRole("heading", { name: limiterTask, exact: true }).waitFor()
  await page.waitForTimeout(1_200)

  await page.keyboard.press("Escape")
  await page.waitForTimeout(400)
  await page.keyboard.press("Escape")
  await page.getByRole("button", { name: /Search/ }).click()
  await page.getByRole("dialog", { name: "Command palette" }).waitFor()
  await page.getByPlaceholder("Search goals, tasks, sessions, skills, workflows…").fill("rate limit")
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
