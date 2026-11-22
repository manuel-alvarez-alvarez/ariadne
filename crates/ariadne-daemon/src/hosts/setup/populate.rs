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

/// Printed by a selection script that installs something. Without it, the
/// step found its item present and reused it.
pub(super) const INSTALLED: &str = "__ARIADNE_INSTALLED__";

// Homebrew refuses root. Require a user-owned prefix before any installation.
// `ensure` prints INSTALLED only when it installs.
const PREREQUISITE: &str = r#"
upgrade() {
    command -v brew >/dev/null 2>&1 || exit 12
    [ "$(id -u)" != 0 ] || exit 12
    prefix=$(brew --prefix)
    [ -O "$prefix" ] && [ -w "$prefix" ] || exit 12
    HOMEBREW_NO_AUTO_UPDATE=1 brew update >/dev/null 2>&1
    if brew list --versions "$1" >/dev/null 2>&1; then
        HOMEBREW_NO_AUTO_UPDATE=1 brew upgrade "$1" >/dev/null 2>&1
    else
        HOMEBREW_NO_AUTO_UPDATE=1 brew install "$1" >/dev/null 2>&1
    fi
}
ensure() {
    if command -v "$1" >/dev/null 2>&1 && "$1" --version >/dev/null 2>&1; then return; fi
    command -v brew >/dev/null 2>&1 || exit 12
    [ "$(id -u)" != 0 ] || exit 12
    prefix=$(brew --prefix)
    [ -O "$prefix" ] && [ -w "$prefix" ] || exit 12
    HOMEBREW_NO_AUTO_UPDATE=1 brew install "$2" >/dev/null 2>&1
    echo __ARIADNE_INSTALLED__
    command -v "$1" >/dev/null 2>&1
    "$1" --version >/dev/null 2>&1
}
"#;

fn npm_package(package: &str, update: bool) -> String {
    let package = quote(package);
    let check =
        format!("npm list --global --prefix \"$HOME/.local\" --depth=0 {package} >/dev/null 2>&1");
    let install =
        format!("npm install --global --prefix \"$HOME/.local\" -- {package} >/dev/null 2>&1");
    if update {
        format!("{install}\n{check}")
    } else {
        format!("{check} || {{ {install}; echo {INSTALLED}; }}\n{check}")
    }
}

impl Hosts {
    /// Whether a run passes over the step: it succeeded, or the user skipped it.
    fn population_done(&self, name: &str, step: &str) -> bool {
        self.0.setup.lock().unwrap()[name]
            .progress
            .steps
            .iter()
            .any(|s| s.name == step && (s.complete || s.skipped))
    }

    /// Whether the user skipped the step. A skipped check never runs again.
    fn population_skipped(&self, name: &str, step: &str) -> bool {
        self.0.setup.lock().unwrap()[name]
            .progress
            .steps
            .iter()
            .any(|s| s.name == step && s.skipped)
    }

    pub(super) fn population_result(
        &self,
        name: &str,
        step: &str,
        result: Result<(), String>,
    ) -> Result<(), String> {
        if name.starts_with("update:") {
            return result;
        }
        if let Err(error) = result {
            let mut records = self.0.setup.lock().unwrap();
            let record = records.get_mut(name).unwrap();
            let step = record
                .progress
                .steps
                .iter_mut()
                .find(|s| s.name == step)
                .unwrap();
            step.complete = false;
            step.error = Some(error);
            save(&self.0.cfg.root, &records)?;
        }
        Ok(())
    }

    pub(super) async fn population_command(
        &self,
        request: &SetupRequest,
        path: &str,
        script: &str,
        input: Option<Vec<u8>>,
    ) -> Result<String, String> {
        self.ssh(
            request,
            &format!("set -eu; umask 077; export PATH={}; {script}", quote(path)),
            input,
        )
        .await
    }

    /// Record whether a successful step reused an item that was already present.
    pub(super) fn found_present(&self, name: &str, step: &str, present: bool) {
        let mut records = self.0.setup.lock().unwrap();
        let record = records.get_mut(name).unwrap();
        if present {
            record.present.insert(step.into());
        } else {
            record.present.remove(step);
        }
    }

    pub(super) async fn populate(
        &self,
        name: &str,
        request: &SetupRequest,
        path: &str,
    ) -> Result<(), String> {
        let update = name.starts_with("update:");
        if !update && request.tools.is_none() {
            let step = "prerequisite:git";
            self.begin(name, step)?;
            let result = self.population_command(request, path,
                "command -v git >/dev/null 2>&1 && git --version >/dev/null 2>&1", None)
                .await
                .map(|_| ())
                .map_err(|_| "Git is missing or unusable on the worker; install Git on the worker and resume setup".to_owned());
            if result.is_ok() {
                self.found_present(name, step, true);
                self.checkpoint(name, step)?;
            }
            self.population_result(name, step, result)?;
        }
        if let Some(tools) = &request.tools {
            for (step, binary, formula) in [("tool:git", "git", "git"), ("tool:github", "gh", "gh")]
            {
                if binary == "gh" && !tools.github {
                    continue;
                }
                if self.population_done(name, step) {
                    continue;
                }
                let result: Result<(), String> = async {
                    self.begin(name, step)?;
                    let action = if update {
                        format!("upgrade {formula}")
                    } else {
                        format!("ensure {binary} {formula}")
                    };
                    let output = self.population_command(request, path, &format!("{PREREQUISITE}\n{action}"), None).await
                        .map_err(|_| format!("missing {binary}; provide user-owned Homebrew or ask an administrator to install this prerequisite"))?;
                    self.found_present(name, step, !update && !output.contains(INSTALLED));
                    self.checkpoint(name, step)?;
                    Ok(())
                }.await;
                self.population_result(name, step, result)?;
            }
            for id in &tools.agents {
                let step = format!("tool:{id}");
                if self.population_done(name, &step) {
                    continue;
                }
                let result: Result<(), String> = async {
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
                        let runtime_command = if update {
                            "upgrade node"
                        } else {
                            "ensure node node"
                        };
                        format!(
                            "{PREREQUISITE}\n{runtime_command}\nensure npm node\nexport npm_config_cache=\"$HOME/.cache/ariadne/npm\"\n{}",
                            npm_package(package, update)
                        )
                    } else if let Some(package) = distribution["uvx"]["package"].as_str() {
                        let runtime_command = if update { "upgrade uv" } else { "ensure uv uv" };
                        let upgrade = if update { " --upgrade" } else { "" };
                        let (tool, version) = package.split_once("==").unwrap_or((package, ""));
                        format!(
                            "{PREREQUISITE}\n{runtime_command}\nexport UV_TOOL_BIN_DIR=\"$HOME/.local/bin\"\nuv tool list 2>/dev/null | grep -Fqx -- {listed} || echo {INSTALLED}\nuv python install >/dev/null 2>&1\nuv tool install{upgrade} -- {package} >/dev/null 2>&1 || uv tool upgrade -- {package} >/dev/null 2>&1",
                            listed = quote(&format!("{tool} v{version}")),
                            package = quote(package)
                        )
                    } else {
                        return Err("unsupported distribution; manual installation and worker login are required".into());
                    };
                    let script = match super::plan::provider_cli(id) {
                        Some(cli) => format!("{script}\n{}", npm_package(cli, update)),
                        None => script,
                    };
                    let output = self.population_command(request, path, &script, None).await.map_err(|_| "agent installation or runtime check failed; provide user-owned Homebrew or ask an administrator for Node/npm or uv prerequisites".to_owned())?;
                    self.found_present(name, &step, !update && !output.contains(INSTALLED));
                    self.checkpoint(name, &step)?;
                    Ok(())
                }.await;
                self.population_result(name, &step, result)?;
            }
        }
        for id in &request.credentials {
            let step = format!("credential:{id}");
            if self.population_done(name, &step) {
                continue;
            }
            let result: Result<(), String> = async {
                self.begin(name, &step)?;
                // Successful transfers are not repeated. Authentication below runs again unless skipped.
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
                Ok(())
            }.await;
            self.population_result(name, &step, result)?;
        }
        if (request.tools.as_ref().is_some_and(|t| t.github)
            || request.credentials.iter().any(|c| c == "github"))
            && !self.population_skipped(name, "authentication:github")
        {
            let result: Result<(), String> = async {
                self.begin(name, "authentication:github")?;
                self.population_command(request, path, "gh auth status --hostname github.com >/dev/null 2>&1 && gh auth setup-git --hostname github.com >/dev/null 2>&1", None).await.map_err(|_| "GitHub authentication missing; run gh auth login separately on the worker")?;
                let copied = request.credentials.iter().any(|c| c == "github");
                self.found_present(name, "authentication:github", !copied);
                self.checkpoint(name, "authentication:github")?;
                Ok(())
            }.await;
            self.population_result(name, "authentication:github", result)?;
        }
        if let Some(tools) = &request.tools {
            for id in &tools.agents {
                let step = format!("authentication:{id}");
                if self.population_skipped(name, &step) {
                    continue;
                }
                let result: Result<(), String> = async {
                    self.begin(name, &step)?;
                    let check = match id.as_str() {
                        "codex-acp" => "codex login status",
                        "claude-acp" => "claude auth status",
                        _ => return Err("agent authentication verification unsupported; sign in separately on the worker".into()),
                    };
                    self.population_command(request, path, &format!("{check} >/dev/null 2>&1"), None).await.map_err(|_| "agent authentication unavailable; install the provider CLI and sign in separately on the worker")?;
                    let copied = id == "codex-acp" && request.credentials.iter().any(|c| c == "codex");
                    self.found_present(name, &step, !copied);
                    self.checkpoint(name, &step)?;
                    Ok(())
                }.await;
                self.population_result(name, &step, result)?;
            }
        }
        Ok(())
    }
}
