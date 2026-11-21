//! Install registry packages and copy explicitly selected credentials over SSH stdin.
use super::*;
use ariadne_api::hosts::SetupTools;

pub(super) fn selection_steps(request: &SetupRequest) -> Vec<String> {
    let mut steps = Vec::new();
    if let Some(SetupTools { agents, github }) = &request.tools {
        steps.push("tool:git".into());
        if *github {
            steps.push("tool:github".into());
        }
        steps.extend(agents.iter().map(|id| format!("tool:{id}")));
    }
    steps.extend(
        request
            .credentials
            .iter()
            .map(|id| format!("credential:{id}")),
    );
    if request.tools.as_ref().is_some_and(|t| t.github)
        || request.credentials.iter().any(|c| c == "github")
    {
        steps.push("authentication:github".into());
    }
    if let Some(tools) = &request.tools {
        steps.extend(tools.agents.iter().map(|id| format!("authentication:{id}")));
    }
    steps
}

// Homebrew refuses root. Require a user-owned prefix before any installation.
const PREREQUISITE: &str = r#"
ensure() {
    if command -v "$1" >/dev/null 2>&1 && "$1" --version >/dev/null 2>&1; then return; fi
    command -v brew >/dev/null 2>&1 || exit 12
    [ "$(id -u)" != 0 ] || exit 12
    prefix=$(brew --prefix)
    [ -O "$prefix" ] && [ -w "$prefix" ] || exit 12
    HOMEBREW_NO_AUTO_UPDATE=1 brew install "$2" >/dev/null 2>&1
    command -v "$1" >/dev/null 2>&1
    "$1" --version >/dev/null 2>&1
}
"#;

impl Hosts {
    async fn population_command(
        &self,
        request: &SetupRequest,
        path: &str,
        script: &str,
        input: Option<Vec<u8>>,
    ) -> Result<(), String> {
        self.ssh(
            request,
            &format!("set -eu; umask 077; export PATH={}; {script}", quote(path)),
            input,
        )
        .await
        .map(|_| ())
    }

    pub(super) async fn populate(
        &self,
        name: &str,
        request: &SetupRequest,
        path: &str,
    ) -> Result<(), String> {
        if let Some(tools) = &request.tools {
            for (step, binary, formula) in [("tool:git", "git", "git"), ("tool:github", "gh", "gh")]
            {
                if binary == "gh" && !tools.github {
                    continue;
                }
                self.begin(name, step)?;
                self.population_command(request, path, &format!("{PREREQUISITE}\nensure {binary} {formula}"), None).await
                    .map_err(|_| format!("missing {binary}; provide user-owned Homebrew or ask an administrator to install this prerequisite"))?;
                self.checkpoint(name, step)?;
            }
            for id in &tools.agents {
                let step = format!("tool:{id}");
                self.begin(name, &step)?;
                // Configured custom commands override registry discovery. Do not infer their installer.
                if self.0.cfg.acp_agents.iter().any(|a| &a.id == id) {
                    return Err("custom agent commands require manual installation; no installation instructions are available".into());
                }
                let index: serde_json::Value =
                    serde_json::from_str(crate::acp_discovery::SHIPPED_INDEX).unwrap();
                let distribution = index["agents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == *id)
                    .map(|a| &a["distribution"]);
                let Some(distribution) = distribution else {
                    return Err(
                        "unsupported agent; manual installation and worker login are required"
                            .into(),
                    );
                };
                let script = if let Some(package) = distribution["npx"]["package"].as_str() {
                    // npm owns only the worker user's prefix. Verify the installed package before each retry.
                    format!(
                        "{PREREQUISITE}\nensure node node\nensure npm node\nexport npm_config_cache=\"$HOME/.cache/ariadne/npm\"\npackage={package}\nnpm list --global --prefix \"$HOME/.local\" --depth=0 \"$package\" >/dev/null 2>&1 || npm install --global --prefix \"$HOME/.local\" -- \"$package\" >/dev/null 2>&1\nnpm list --global --prefix \"$HOME/.local\" --depth=0 \"$package\" >/dev/null 2>&1",
                        package = quote(package)
                    )
                } else if let Some(package) = distribution["uvx"]["package"].as_str() {
                    format!(
                        "{PREREQUISITE}\nensure uv uv\nexport UV_TOOL_BIN_DIR=\"$HOME/.local/bin\"\nuv python install >/dev/null 2>&1\nuv tool install -- {package} >/dev/null 2>&1 || uv tool upgrade -- {package} >/dev/null 2>&1",
                        package = quote(package)
                    )
                } else {
                    return Err("unsupported distribution; manual installation and worker login are required".into());
                };
                let script = match id.as_str() {
                    "codex-acp" => format!(
                        "{script}\nnpm list --global --prefix \"$HOME/.local\" --depth=0 @openai/codex >/dev/null 2>&1 || npm install --global --prefix \"$HOME/.local\" -- @openai/codex >/dev/null 2>&1"
                    ),
                    "claude-acp" => format!(
                        "{script}\nnpm list --global --prefix \"$HOME/.local\" --depth=0 @anthropic-ai/claude-code >/dev/null 2>&1 || npm install --global --prefix \"$HOME/.local\" -- @anthropic-ai/claude-code >/dev/null 2>&1"
                    ),
                    _ => script,
                };
                self.population_command(request, path, &script, None).await.map_err(|_| "agent installation or runtime check failed; provide user-owned Homebrew or ask an administrator for Node/npm or uv prerequisites".to_owned())?;
                self.checkpoint(name, &step)?;
            }
        }
        for id in &request.credentials {
            let step = format!("credential:{id}");
            if self.0.setup.lock().unwrap()[name]
                .progress
                .steps
                .iter()
                .any(|s| s.name == step && s.complete)
            {
                continue;
            }
            self.begin(name, &step)?;
            // Successful transfers are not repeated. Authentication below always runs again.
            // A checkpoint is durable only after SSH confirms the write or login.
            match id.as_str() {
                "codex" => {
                    let source = self.0.cfg.native_home.join(".codex/auth.json");
                    let data = std::fs::read(source).map_err(
                        |_| "Codex file credentials unavailable; sign in separately on the worker",
                    )?;
                    serde_json::from_slice::<serde_json::Value>(&data).map_err(
                        |_| "Codex file credentials invalid; sign in separately on the worker",
                    )?;
                    self.population_command(request, path, r#"mkdir -p "$HOME/.codex"; chmod 700 "$HOME/.codex"; temp=$(mktemp "$HOME/.codex/auth.XXXXXXXX"); trap 'rm -f "$temp"' EXIT HUP INT TERM; cat > "$temp"; chmod 600 "$temp"; mv -f "$temp" "$HOME/.codex/auth.json""#, Some(data)).await
                        .map_err(|_| "Codex credential transfer failed; retry setup or sign in separately on the worker")?;
                }
                "github" => {
                    let gh = self.0.cfg.gh_bin.as_deref().unwrap_or("gh");
                    let token = self.command(Command::new(gh).args(["auth", "token", "--hostname", "github.com"]), None).await.map_err(|_| "GitHub source credentials unavailable; sign in separately on the worker")?;
                    if token.trim().is_empty() {
                        return Err("GitHub source credentials unavailable; sign in separately on the worker".into());
                    }
                    self.population_command(request, path, "gh auth login --hostname github.com --git-protocol https --with-token >/dev/null 2>&1", Some(token.into_bytes())).await.map_err(|_| "GitHub credential transfer failed; sign in separately on the worker")?;
                }
                _ => {
                    return Err(
                        "unsupported credential transfer; sign in separately on the worker".into(),
                    );
                }
            }
            self.checkpoint(name, &step)?;
        }
        if request.tools.as_ref().is_some_and(|t| t.github)
            || request.credentials.iter().any(|c| c == "github")
        {
            self.begin(name, "authentication:github")?;
            self.population_command(request, path, "gh auth status --hostname github.com >/dev/null 2>&1 && gh auth setup-git --hostname github.com >/dev/null 2>&1", None).await.map_err(|_| "GitHub authentication missing; run gh auth login separately on the worker")?;
            self.checkpoint(name, "authentication:github")?;
        }
        if let Some(tools) = &request.tools {
            for id in &tools.agents {
                let step = format!("authentication:{id}");
                self.begin(name, &step)?;
                let check = match id.as_str() {
                    "codex-acp" => "codex login status",
                    "claude-acp" => "claude auth status",
                    _ => return Err("agent authentication verification unsupported; sign in separately on the worker".into()),
                };
                self.population_command(request, path, &format!("{check} >/dev/null 2>&1"), None).await.map_err(|_| "agent authentication unavailable; install the provider CLI and sign in separately on the worker")?;
                self.checkpoint(name, &step)?;
            }
        }
        Ok(())
    }
}
