//! Read-only worker inspection and a review of explicitly selected actions.
use super::build::BUILD_PROBE;
use super::plan::{
    Install, agent_name, agent_sign_in, build_manual, index, install, manual_for, packages,
    packages_script, sign_ins, title,
};
use super::*;
use ariadne_api::hosts::{
    InspectRequest, InspectedAgent, InspectedGithub, InspectionDto, ManualAction, PlanItem,
    SetupCheck, SetupGroup, SetupReviewDto,
};

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
        local_agents: &[String],
    ) -> Result<InspectionDto, String> {
        Ok(self.inspect(name, request, local_agents).await?.0)
    }

    /// The inspection, and the worker's system package manager where one was found.
    async fn inspect(
        &self,
        name: &str,
        request: InspectRequest,
        local_agents: &[String],
    ) -> Result<(InspectionDto, Option<String>), String> {
        let request = connection(request.ssh);
        validate(name, &request)?;
        // Suppress all provider output. Only fixed capability identifiers leave the worker.
        let script = r#"set -eu
os=$(uname -s)
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
"#;
        let script = format!(
            "{script}{BUILD_PROBE}{}exit 0\n",
            packages_script(&self.0.cfg)
        );
        let output = self.ssh(&request, &script, None).await.map_err(|_| {
            "Inspection failed. Check the worker account, SSH keys, and configured host trust."
                .to_owned()
        })?;
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
        let manager = available
            .iter()
            .find_map(|line| line.strip_prefix("manager:"))
            .map(String::from);
        let mut checks = Vec::new();
        for tool in [
            "git", "tar", "gh", "node", "npm", "uv", "codex", "claude", "brew",
        ] {
            checks.push(check(tool, available.contains(&tool), &format!("{tool} is unavailable."), "Install the program on the worker. Supported prerequisites can use existing user-owned Homebrew."));
        }
        checks.push(check(
            "compiler",
            available.contains(&"compiler"),
            "A C program does not compile and link.",
            "Ask an administrator to install a C compiler, a linker and the C library headers. A source build needs them.",
        ));
        checks.push(check(
            "cargo",
            available.contains(&"cargo"),
            "Rust is unavailable.",
            "A source build installs Rust with rustup in the user's home directory.",
        ));
        checks.push(check(
            "rustup",
            available.contains(&"rustup"),
            "rustup is unavailable.",
            "A source build downloads rustup with curl when Rust is missing.",
        ));
        checks.push(check(
            "curl",
            available.contains(&"curl"),
            "curl is unavailable.",
            "Ask an administrator to install curl. A source build needs it to install Rust.",
        ));
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
        let index = index();
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
        let os = if target.ends_with("apple-darwin") {
            "Darwin"
        } else {
            "Linux"
        };
        let has = |tool: &str| available.contains(&tool);
        let brew = has("brew");
        let mut ids: Vec<String> = index["agents"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["id"].as_str().map(String::from))
            .collect();
        for custom in &self.0.cfg.acp_agents {
            if !ids.contains(&custom.id) {
                ids.push(custom.id.clone());
            }
        }
        let agents = ids
            .into_iter()
            .map(|id| {
                let present = packages(&self.0.cfg, &id)
                    .is_some_and(|all| all.iter().all(|p| has(&format!("package:{p}"))));
                let installs = match install(&self.0.cfg, &id) {
                    Install::Npm => brew || (has("node") && has("npm")),
                    Install::Uv => brew || has("uv"),
                    Install::Custom | Install::Unsupported => false,
                };
                let key = format!("tool:{id}");
                let group = if present {
                    SetupGroup::AlreadyPresent
                } else if installs {
                    SetupGroup::AriadneInstalls
                } else {
                    SetupGroup::Manual
                };
                InspectedAgent {
                    name: agent_name(&id),
                    on_this_machine: local_agents.contains(&id),
                    group,
                    manual: (group == SetupGroup::Manual)
                        .then(|| manual_for(&self.0.cfg, &key, Some(os)))
                        .flatten(),
                    sign_in: agent_sign_in(&id).map(String::from),
                    id,
                }
            })
            .collect();
        let github = if has("gh") {
            InspectedGithub {
                group: SetupGroup::AlreadyPresent,
                manual: None,
            }
        } else if brew {
            InspectedGithub {
                group: SetupGroup::AriadneInstalls,
                manual: None,
            }
        } else {
            InspectedGithub {
                group: SetupGroup::Manual,
                manual: manual_for(&self.0.cfg, "tool:github", Some(os)),
            }
        };
        Ok((
            InspectionDto {
                binaries: self.0.cfg.build.binaries(),
                target: target.into(),
                checks,
                supported_agents,
                supported_credentials: vec!["codex".into(), "github".into()],
                agents,
                github,
                sign_ins: sign_ins(),
            },
            manager,
        ))
    }

    pub(crate) async fn review_setup(
        &self,
        name: &str,
        request: SetupRequest,
        local_agents: &[String],
    ) -> Result<SetupReviewDto, String> {
        validate(name, &request)?;
        let (inspection, manager) = self
            .inspect(
                name,
                InspectRequest {
                    ssh: request.ssh.clone(),
                },
                local_agents,
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
            BinarySource::Daemon if inspection.binaries.tag.is_some() => {
                actions.push(format!(
                    "Install Ariadne {} with a verified release.",
                    inspection.binaries.version
                ));
            }
            BinarySource::Daemon => {
                let build = &self.0.cfg.build;
                let source = build.check_source().await;
                prerequisites.push(check(
                    "binaries",
                    source.is_ok(),
                    source.as_ref().err().map_or(
                        "The checkout or the commit of this daemon is missing on this machine.",
                        String::as_str,
                    ),
                    "Restore the checkout and its commit on this machine, or select an explicit local or release source.",
                ));
                for id in ["compiler", "cargo", "rustup", "curl"] {
                    prerequisites.push(
                        inspection
                            .checks
                            .iter()
                            .find(|c| c.id == id)
                            .unwrap()
                            .clone(),
                    );
                }
                actions.push(format!(
                    "Install Ariadne {} with a source build of commit {} on the new machine.",
                    inspection.binaries.version, build.commit
                ));
                if build.uncommitted {
                    actions.push(UNCOMMITTED.into());
                }
            }
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
        let plan = self.plan(&request, &inspection, &prerequisites, manager.as_deref());
        Ok(SetupReviewDto {
            plan,
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

const UNCOMMITTED: &str = "This daemon was built with uncommitted edits. The new machine gets the commit only, without those edits.";

fn item(
    id: String,
    group: SetupGroup,
    detail: impl Into<String>,
    manual: Option<ManualAction>,
) -> PlanItem {
    PlanItem {
        title: title(&id),
        manual: (group == SetupGroup::Manual).then_some(manual).flatten(),
        id,
        group,
        detail: detail.into(),
    }
}

impl Hosts {
    /// One item for each step the request causes, in the order setup runs them.
    fn plan(
        &self,
        request: &SetupRequest,
        inspection: &InspectionDto,
        prerequisites: &[SetupCheck],
        manager: Option<&str>,
    ) -> Vec<PlanItem> {
        let cfg = &self.0.cfg;
        let os = Some(if inspection.target.ends_with("apple-darwin") {
            "Darwin"
        } else {
            "Linux"
        });
        let manual = |id: &str| manual_for(cfg, id, os);
        let present = |id: &str| inspection.checks.iter().any(|c| c.id == id && c.satisfied);
        let met = |id: &str| prerequisites.iter().any(|c| c.id == id && c.satisfied);
        let mut plan = Vec::new();
        plan.push(match &request.source {
            BinarySource::Daemon if inspection.binaries.tag.is_some() => {
                let info = &inspection.binaries;
                item(
                    "install".into(),
                    SetupGroup::AriadneInstalls,
                    format!(
                        "Install Ariadne {} with a verified release ({}).",
                        info.version,
                        info.tag.as_deref().unwrap_or_default()
                    ),
                    None,
                )
            }
            BinarySource::Daemon => {
                let info = &inspection.binaries;
                let commit = info.commit.as_deref().unwrap_or("unknown");
                let mut detail = format!(
                    "Install Ariadne {} with a source build: build commit {commit} on the new machine, then install it in ~/.local/bin.",
                    info.version
                );
                if info.uncommitted {
                    detail = format!("{detail} {UNCOMMITTED}");
                }
                let refusal = prerequisites
                    .iter()
                    .find(|c| c.id == "binaries" && !c.satisfied)
                    .map(|c| ManualAction {
                        reason: c.missing.clone(),
                        command: None,
                    });
                item(
                    "install".into(),
                    if refusal.is_some() {
                        SetupGroup::Manual
                    } else {
                        SetupGroup::AriadneInstalls
                    },
                    detail,
                    refusal,
                )
            }
            BinarySource::Local { .. } if met("binaries") => item(
                "install".into(),
                SetupGroup::AriadneInstalls,
                format!("Copy ariadne and ariadned from this machine to ~/.local/bin. Both run on {}.", inspection.target),
                None,
            ),
            BinarySource::Local { .. } => item(
                "install".into(),
                SetupGroup::Manual,
                "Ariadne cannot use the selected programs.",
                manual("install"),
            ),
            BinarySource::Release { repository, tag } => item(
                "install".into(),
                SetupGroup::AriadneInstalls,
                format!("Download release {tag} of {repository}, verify it, and install it in ~/.local/bin."),
                None,
            ),
        });
        if self.source_build(request, true) {
            plan.extend(self.build_plan(present, os, manager));
        }
        let git = if request.tools.is_some() {
            "tool:git"
        } else {
            "prerequisite:git"
        };
        plan.push(if present("git") {
            item(
                git.into(),
                SetupGroup::AlreadyPresent,
                "Git is already on the new machine.",
                None,
            )
        } else if request.tools.is_some() && present("brew") {
            item(
                git.into(),
                SetupGroup::AriadneInstalls,
                "Install Git with the user-owned Homebrew.",
                None,
            )
        } else {
            item(
                git.into(),
                SetupGroup::Manual,
                "Git is not on the new machine.",
                manual(git),
            )
        });
        if let Some(tools) = &request.tools {
            if tools.github {
                let github = &inspection.github;
                let detail = match github.group {
                    SetupGroup::AlreadyPresent => "GitHub CLI is already on the new machine.",
                    SetupGroup::AriadneInstalls => {
                        "Install GitHub CLI with the user-owned Homebrew."
                    }
                    SetupGroup::Manual => "GitHub CLI is not on the new machine.",
                };
                plan.push(item(
                    "tool:github".into(),
                    github.group,
                    detail,
                    github.manual.clone(),
                ));
            }
            for id in &tools.agents {
                let key = format!("tool:{id}");
                let name = agent_name(id);
                let (group, manual) = inspection
                    .agents
                    .iter()
                    .find(|a| a.id == *id)
                    .map_or((SetupGroup::Manual, manual(&key)), |a| {
                        (a.group, a.manual.clone())
                    });
                let detail = match group {
                    SetupGroup::AriadneInstalls => {
                        format!("Install {name} from the shipped agent registry.")
                    }
                    SetupGroup::AlreadyPresent => {
                        format!("{name} is already on the new machine. Setup reuses it.")
                    }
                    SetupGroup::Manual => format!("Ariadne cannot install {name}."),
                };
                plan.push(item(key, group, detail, manual));
            }
        }
        let mut copied = Vec::new();
        for id in &request.credentials {
            let key = format!("credential:{id}");
            let available = met(&format!("source-credential:{id}"));
            plan.push(match id.as_str() {
                "codex" if available => item(
                    key,
                    SetupGroup::AriadneInstalls,
                    "Copy ~/.codex/auth.json from this machine with mode 0600.",
                    None,
                ),
                "github" if available => item(
                    key,
                    SetupGroup::AriadneInstalls,
                    "Copy the GitHub token from this machine, and set up Git to use it.",
                    None,
                ),
                _ => item(
                    key.clone(),
                    SetupGroup::Manual,
                    "Ariadne copies no sign-in for this selection.",
                    manual(&key),
                ),
            });
            if available {
                copied.push(id.as_str());
            }
        }
        let sign_in = |key: String, copy: &str, file: &str, name: &str| {
            if copied.contains(&copy) {
                item(
                    key,
                    SetupGroup::AriadneInstalls,
                    format!("Check the {name} sign-in after Ariadne copies it."),
                    None,
                )
            } else if present(file) {
                item(
                    key,
                    SetupGroup::AlreadyPresent,
                    format!("{name} is already signed in on the new machine. Setup checks it."),
                    None,
                )
            } else {
                let manual = manual(&key);
                item(
                    key,
                    SetupGroup::Manual,
                    format!("{name} is not signed in on the new machine."),
                    manual,
                )
            }
        };
        if request.tools.as_ref().is_some_and(|t| t.github)
            || request.credentials.iter().any(|c| c == "github")
        {
            plan.push(sign_in(
                "authentication:github".into(),
                "github",
                "credential-file:github",
                "GitHub CLI",
            ));
        }
        for id in request.tools.iter().flat_map(|t| &t.agents) {
            let key = format!("authentication:{id}");
            plan.push(match id.as_str() {
                "codex-acp" => sign_in(key, "codex", "credential-file:codex", "Codex"),
                "claude-acp" => sign_in(key, "claude", "credential-file:claude", "Claude"),
                _ => {
                    let manual = manual(&key);
                    item(
                        key,
                        SetupGroup::Manual,
                        format!("Ariadne cannot check the sign-in of {}.", agent_name(id)),
                        manual,
                    )
                }
            });
        }
        for (id, destination) in &request.repositories {
            plan.push(if met(&format!("repository:{id}")) {
                PlanItem {
                    title: format!("Clone and register repository {id}"),
                    ..item(format!("repository:{id}"), SetupGroup::AriadneInstalls, format!("Clone {id} from its Git remote into {destination}, then register it. Local edits stay on this machine."), None)
                }
            } else {
                PlanItem {
                    title: format!("Clone and register repository {id}"),
                    ..item(format!("repository:{id}"), SetupGroup::Manual, format!("Repository {id} is not registered on this machine."), Some(ManualAction {
                        reason: format!("Repository {id} is not registered on this machine. Select a registered repository, then review again."),
                        command: None,
                    }))
                }
            });
        }
        let service = if os == Some("Darwin") {
            "launchd"
        } else {
            "systemd"
        };
        plan.push(if present("service") {
            item(
                "service".into(),
                SetupGroup::AriadneInstalls,
                format!("Register the Ariadne service with {service} and start it."),
                None,
            )
        } else {
            item(
                "service".into(),
                SetupGroup::Manual,
                format!("The {service} user service is not ready on the new machine."),
                manual("service"),
            )
        });
        plan.push(item(
            "register".into(),
            SetupGroup::AriadneInstalls,
            "Connect this daemon to the new machine, then copy skills, workflows, permissions and portable settings.",
            None,
        ));
        plan
    }
}

impl Hosts {
    /// The items of a source build on the worker, in the order setup runs them.
    fn build_plan(
        &self,
        present: impl Fn(&str) -> bool,
        os: Option<&str>,
        manager: Option<&str>,
    ) -> Vec<PlanItem> {
        let build = &self.0.cfg.build;
        let manual = |id: &str| build_manual(id, os, manager);
        vec![
            if present("compiler") {
                item(
                    "prerequisite:compiler".into(),
                    SetupGroup::AlreadyPresent,
                    "A C program compiles and links on the new machine.",
                    None,
                )
            } else {
                item(
                    "prerequisite:compiler".into(),
                    SetupGroup::Manual,
                    "A C program does not compile and link on the new machine.",
                    manual("prerequisite:compiler"),
                )
            },
            if present("cargo") {
                item(
                    "prerequisite:curl".into(),
                    SetupGroup::AlreadyPresent,
                    "Rust is already on the new machine, so the build needs no curl.",
                    None,
                )
            } else if present("rustup") {
                item(
                    "prerequisite:curl".into(),
                    SetupGroup::AlreadyPresent,
                    "rustup is already on the new machine, so the build needs no curl.",
                    None,
                )
            } else if present("curl") {
                item(
                    "prerequisite:curl".into(),
                    SetupGroup::AlreadyPresent,
                    "curl is already on the new machine.",
                    None,
                )
            } else {
                item(
                    "prerequisite:curl".into(),
                    SetupGroup::Manual,
                    "The new machine has no curl to install Rust with.",
                    manual("prerequisite:curl"),
                )
            },
            item(
                "source".into(),
                SetupGroup::AriadneInstalls,
                format!(
                    "Pack commit {} from {} on this machine and copy it to ~/.ariadne/worker-source on the new machine. Uncommitted edits and untracked files stay on this machine.",
                    build.commit, build.checkout
                ),
                None,
            ),
            if present("cargo") {
                item(
                    "rust".into(),
                    SetupGroup::AlreadyPresent,
                    "Rust is already on the new machine. Setup uses it.",
                    None,
                )
            } else {
                item(
                    "rust".into(),
                    SetupGroup::AriadneInstalls,
                    "Install Rust with rustup in the home directory of the user on the new machine.",
                    None,
                )
            },
            item(
                "build".into(),
                SetupGroup::AriadneInstalls,
                "Build ariadne and ariadned in release mode on the new machine. A build of the same commit that completed is used again.",
                None,
            ),
        ]
    }
}
