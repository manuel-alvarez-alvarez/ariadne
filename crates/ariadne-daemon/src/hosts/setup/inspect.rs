//! Read-only worker inspection and a review of explicitly selected actions.
use super::*;
use ariadne_api::hosts::{InspectRequest, InspectionDto, SetupCheck, SetupReviewDto};

fn check(id: &str, satisfied: bool, missing: &str, resolution: &str) -> SetupCheck {
    SetupCheck {
        id: id.into(),
        satisfied,
        missing: missing.into(),
        resolution: resolution.into(),
        recheck: "Inspect again, review the selections, then retry the original setup request."
            .into(),
    }
}

fn connection(ssh: String) -> SetupRequest {
    SetupRequest {
        ssh,
        remote: None,
        source: BinarySource::Local {
            cli: String::new(),
            daemon: String::new(),
        },
        tools: None,
        credentials: vec![],
        repositories: BTreeMap::new(),
    }
}

impl Hosts {
    pub(crate) async fn inspect_setup(
        &self,
        name: &str,
        request: InspectRequest,
    ) -> Result<InspectionDto, String> {
        let request = connection(request.ssh);
        validate(name, &request)?;
        // Suppress all provider output. Only fixed capability identifiers leave the worker.
        let output = self.ssh(&request, r#"set -eu
uname -s
uname -m
marker=__ARIADNE_INSPECT_PATH__
if remote_path=$("${SHELL:-/bin/sh}" -lc 'printf "__ARIADNE_INSPECT_PATH__%s__ARIADNE_INSPECT_PATH__" "$PATH"' 2>/dev/null); then
    case "$remote_path" in
        *"$marker"*"$marker"*) remote_path=${remote_path#*"$marker"}; remote_path=${remote_path%%"$marker"*};
            if [ -n "$remote_path" ]; then PATH=$remote_path; fi;;
    esac
fi
export PATH="$HOME/.local/bin:$PATH"
for tool in git tar gh node npm uv codex claude; do
    if command -v "$tool" >/dev/null 2>&1; then printf '%s\n' "$tool"; fi
done
if command -v brew >/dev/null 2>&1 && [ "$(id -u)" != 0 ]; then
    prefix=$(brew --prefix 2>/dev/null) || prefix=
    if [ -n "$prefix" ] && [ -O "$prefix" ] && [ -w "$prefix" ]; then echo brew; fi
fi
case $(uname -s) in
Linux)
    if command -v systemctl >/dev/null 2>&1 && command -v loginctl >/dev/null 2>&1 && [ "$(loginctl show-user "$(id -u)" -p Linger --value 2>/dev/null)" = yes ] && systemctl --user show-environment >/dev/null 2>&1; then echo service; fi;;
Darwin) if launchctl print "user/$(id -u)" >/dev/null 2>&1 || launchctl print "gui/$(id -u)" >/dev/null 2>&1; then echo service; fi;;
esac
if [ -s "${XDG_CONFIG_HOME:-$HOME/.config}/gh/hosts.yml" ]; then echo credential-file:github; fi
if [ -s "$HOME/.codex/auth.json" ]; then echo credential-file:codex; fi
if [ -s "$HOME/.claude/.credentials.json" ]; then echo credential-file:claude; fi
exit 0
"#, None).await.map_err(|_| "Inspection failed. Check the worker account, SSH keys, and configured host trust.".to_owned())?;
        let mut lines = output.lines();
        let target = match (lines.next(), lines.next()) {
            (Some("Linux"), Some("x86_64")) => "x86_64-unknown-linux-gnu",
            (Some("Linux"), Some("arm64" | "aarch64")) => "aarch64-unknown-linux-gnu",
            (Some("Darwin"), Some("x86_64")) => "x86_64-apple-darwin",
            (Some("Darwin"), Some("arm64" | "aarch64")) => "aarch64-apple-darwin",
            _ => {
                return Err(
                    "Unsupported worker platform. Use macOS or Linux on ARM64 or x86_64.".into(),
                );
            }
        };
        let available: Vec<_> = lines.collect();
        let mut checks = Vec::new();
        for tool in [
            "git", "tar", "gh", "node", "npm", "uv", "codex", "claude", "brew",
        ] {
            checks.push(check(tool, available.contains(&tool), &format!("{tool} is unavailable."), "Install the program on the worker. Supported prerequisites can use existing user-owned Homebrew."));
        }
        checks.push(check("service", available.contains(&"service"), "The user service is unavailable.", "On Linux, ask the administrator for systemd user services and lingering. On macOS, check the launchd user session."));
        for id in ["github", "codex-acp", "claude-acp"] {
            let key = format!("authentication:{id}");
            checks.push(check(
                &key,
                available.contains(&key.as_str()),
                "Worker authentication needs verification during setup.",
                match id {
                    "github" => "Run gh auth login on the worker.",
                    "codex-acp" => "Run codex login on the worker.",
                    _ => "Run claude auth login on the worker.",
                },
            ));
        }
        for id in ["codex", "claude", "github"] {
            let key = format!("credential-file:{id}");
            checks.push(check(
                &key,
                available.contains(&key.as_str()),
                "No worker credential file was found.",
                "Sign in on the worker. Setup verifies authentication after installation.",
            ));
        }
        let index: serde_json::Value =
            serde_json::from_str(crate::acp_discovery::SHIPPED_INDEX).unwrap();
        let supported_agents = index["agents"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|a| {
                (a["distribution"]["npx"]["package"].is_string()
                    || a["distribution"]["uvx"]["package"].is_string())
                    && !self.0.cfg.acp_agents.iter().any(|c| a["id"] == c.id)
            })
            .filter_map(|a| a["id"].as_str().map(String::from))
            .collect();
        Ok(InspectionDto {
            target: target.into(),
            checks,
            supported_agents,
            supported_credentials: vec!["codex".into(), "github".into()],
        })
    }

    pub(crate) async fn review_setup(
        &self,
        name: &str,
        request: SetupRequest,
    ) -> Result<SetupReviewDto, String> {
        validate(name, &request)?;
        let inspection = self
            .inspect_setup(
                name,
                InspectRequest {
                    ssh: request.ssh.clone(),
                },
            )
            .await?;
        let mut actions = vec!["Install Ariadne binaries in ~/.local/bin and configure the user service.".into(), "Synchronize registered skill bundles, workflows, permissions, and portable settings after registration.".into()];
        let mut prerequisites: Vec<_> = inspection
            .checks
            .iter()
            .filter(|c| matches!(c.id.as_str(), "tar" | "service"))
            .cloned()
            .collect();
        match &request.source {
            BinarySource::Local { cli, daemon } => {
                let valid = [cli, daemon]
                    .iter()
                    .all(|p| std::fs::read(p).is_ok_and(|b| compatible(&b, &inspection.target)));
                prerequisites.push(check(
                    "binaries",
                    valid,
                    "Local binaries are missing or incompatible.",
                    "Select both compatible executable files on the source daemon machine.",
                ));
                actions.push(
                    "Copy the two selected local binaries after checking executable compatibility."
                        .into(),
                );
            }
            BinarySource::Release { repository, tag } => actions.push(format!(
                "Download and verify release {repository} at {tag} before transfer."
            )),
        }
        if let Some(tools) = &request.tools {
            actions.push(
                "Ensure Git is installed with existing user-owned Homebrew when necessary.".into(),
            );
            if tools.github {
                actions
                    .push("Ensure GitHub CLI is installed and check worker authentication.".into());
            }
            for id in &tools.agents {
                if inspection.supported_agents.contains(id) {
                    actions.push(format!("Install the shipped npm or uv package for {id}."));
                } else {
                    prerequisites.push(check(&format!("tool:{id}"), false, "Automatic installation is unsupported.", "Install the configured command on the worker. Review the worker configuration before retrying."));
                }
                let key = format!("authentication:{id}");
                prerequisites.push(inspection.checks.iter().find(|c| c.id == key).cloned().unwrap_or_else(|| check(&key, false, "Authentication verification is unsupported.", "Sign in on the worker. Use a supported authentication check before setup can complete.")));
            }
        }
        if request.tools.as_ref().is_some_and(|t| t.github)
            || request.credentials.iter().any(|c| c == "github")
        {
            prerequisites.push(
                inspection
                    .checks
                    .iter()
                    .find(|c| c.id == "authentication:github")
                    .unwrap()
                    .clone(),
            );
        }
        let mut required = vec!["git"];
        if let Some(tools) = &request.tools {
            if tools.github {
                required.push("gh");
            }
            let index: serde_json::Value =
                serde_json::from_str(crate::acp_discovery::SHIPPED_INDEX).unwrap();
            for id in &tools.agents {
                if let Some(agent) = index["agents"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == *id)
                {
                    if agent["distribution"]["npx"]["package"].is_string() {
                        required.extend(["node", "npm"]);
                    }
                    if agent["distribution"]["uvx"]["package"].is_string() {
                        required.push("uv");
                    }
                }
            }
        }
        if request.credentials.iter().any(|c| c == "github") {
            required.push("gh");
        }
        required.sort_unstable();
        required.dedup();
        for id in required {
            prerequisites.push(
                inspection
                    .checks
                    .iter()
                    .find(|c| c.id == id)
                    .unwrap()
                    .clone(),
            );
        }
        for id in &request.credentials {
            if id == "codex" {
                let available = std::fs::read(self.0.cfg.native_home.join(".codex/auth.json"))
                    .is_ok_and(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
                prerequisites.push(check("source-credential:codex", available, "Selected Codex file credentials are unavailable.", "Sign in on the source daemon machine with file credentials, or sign in separately on the worker."));
            } else if id == "github" {
                let gh = self.0.cfg.gh_bin.as_deref().unwrap_or("gh");
                let available = self
                    .command(
                        Command::new(gh).args(["auth", "status", "--hostname", "github.com"]),
                        None,
                    )
                    .await
                    .is_ok();
                prerequisites.push(check("source-credential:github", available, "Source GitHub authentication is unavailable.", "Run gh auth login on the source daemon machine, or sign in separately on the worker."));
            } else {
                prerequisites.push(check(
                    &format!("credential:{id}"),
                    false,
                    "This credential transfer is unsupported.",
                    "Use a supported credential selection. Sign in separately on the worker.",
                ));
            }
            actions.push(match id.as_str() { "codex" => "Copy selected Codex .codex/auth.json through SSH stdin with mode 0600.".into(), "github" => "Copy the selected GitHub token through SSH stdin and configure Git authentication.".into(), _ => format!("Credential {id} requires a separate worker login. No credential will be copied.") });
        }
        for (id, destination) in &request.repositories {
            let exists = self.0.store.get_repository(id).await.is_ok();
            prerequisites.push(check(
                &format!("repository:{id}"),
                exists,
                "The selected source repository is missing.",
                "Select a registered source repository with an accessible Git remote.",
            ));
            actions.push(format!("Clone repository {id} into {destination}, or reuse its registered forge identity. Copy no local edits."));
        }
        Ok(SetupReviewDto {
            inspection,
            actions,
            prerequisites,
            preserved: vec![
                "Preserve machine-specific paths, socket, service definitions, and worker data."
                    .into(),
                "Preserve worker-only catalog entries and repository destinations.".into(),
            ],
        })
    }
}
