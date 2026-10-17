#!/usr/bin/env bash
# Seed a small login HTTP service into the git repository at $1, and commit
# it. Shared by both README demo recordings so they show the same codebase:
# assets/demo.tape (vhs) and assets/record-demo-ui.mjs (Playwright). The
# caller has already run `git init` there.
set -euo pipefail

repo="$1"

mkdir -p "$repo/src/routes"

cat > "$repo/package.json" <<'EOF'
{
  "name": "login-service",
  "private": true,
  "version": "1.0.0",
  "description": "A small HTTP service with a login endpoint.",
  "main": "src/server.js"
}
EOF

cat > "$repo/src/server.js" <<'EOF'
const http = require("node:http")
const { login } = require("./routes/login")

const server = http.createServer((req, res) => {
  if (req.method === "POST" && req.url === "/login") return login(req, res)
  res.writeHead(404, { "content-type": "application/json" })
  res.end(JSON.stringify({ error: "not found" }))
})

server.listen(process.env.PORT || 3000)

module.exports = { server }
EOF

cat > "$repo/src/routes/login.js" <<'EOF'
function login(req, res) {
  let body = ""
  req.on("data", (chunk) => {
    body += chunk
  })
  req.on("end", () => {
    const { username, password } = JSON.parse(body || "{}")
    if (username === "demo" && password === "demo") {
      res.writeHead(200, { "content-type": "application/json" })
      return res.end(JSON.stringify({ token: "demo-token" }))
    }
    res.writeHead(401, { "content-type": "application/json" })
    res.end(JSON.stringify({ error: "invalid credentials" }))
  })
}

module.exports = { login }
EOF

git -C "$repo" add -A
git -C "$repo" -c user.name=Demo -c user.email=demo@example.invalid \
  commit -q -m "chore: seed the login service"
