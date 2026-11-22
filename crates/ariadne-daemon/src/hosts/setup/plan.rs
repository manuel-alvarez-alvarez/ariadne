//! Plain titles and manual work for setup items. Review and progress share them,
//! so a step that waits for the user says what its plan item says.
use super::*;
use ariadne_api::hosts::{ManualAction, SetupGroup, SignIn};

const GITHUB_LOGIN: &str = "gh auth login --hostname github.com";

/// How Ariadne gets one registry or configured agent onto a worker.
pub(super) enum Install {
    /// A command from this daemon's configuration. Nobody can infer its installer.
    Custom,
    Npm,
    Uv,
    /// A binary download, or an ID the registry does not know.
    Unsupported,
}

pub(super) fn index() -> serde_json::Value {
    serde_json::from_str(crate::acp_discovery::SHIPPED_INDEX).unwrap()
}

pub(super) fn install(cfg: &Config, id: &str) -> Install {
    if cfg.acp_agents.iter().any(|a| a.id == id) {
        return Install::Custom;
    }
    let index = index();
    let Some(agent) = index["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
    else {
        return Install::Unsupported;
    };
    if agent["distribution"]["npx"]["package"].is_string() {
        Install::Npm
    } else if agent["distribution"]["uvx"]["package"].is_string() {
        Install::Uv
    } else {
        Install::Unsupported
    }
}

/// The npm or uv packages that setup installs for an agent, as setup names
/// them, or `None` for an agent that setup cannot install.
pub(super) fn packages(cfg: &Config, id: &str) -> Option<Vec<String>> {
    let index = index();
    let agent = index["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)?;
    let package = match install(cfg, id) {
        Install::Npm => agent["distribution"]["npx"]["package"].as_str()?,
        Install::Uv => agent["distribution"]["uvx"]["package"].as_str()?,
        Install::Custom | Install::Unsupported => return None,
    };
    let mut all = vec![package.to_owned()];
    all.extend(provider_cli(id).map(String::from));
    Some(all)
}

/// The provider CLI that setup installs beside an agent's ACP package.
pub(super) fn provider_cli(id: &str) -> Option<&'static str> {
    match id {
        "codex-acp" => Some("@openai/codex"),
        "claude-acp" => Some("@anthropic-ai/claude-code"),
        _ => None,
    }
}

/// The provider CLI's command on the PATH.
pub(super) fn provider_command(id: &str) -> Option<&'static str> {
    match id {
        "codex-acp" => Some("codex"),
        "claude-acp" => Some("claude"),
        _ => None,
    }
}

/// An npm or uv package without its version: `@scope/name@1` and `name==1` give the name.
pub(super) fn package_name(package: &str) -> &str {
    let name = package.split("==").next().unwrap_or(package);
    match name.get(1..).and_then(|rest| rest.find('@')) {
        Some(at) => &name[..at + 1],
        None => name,
    }
}

/// The executable that each npm or uv registry agent installs on the PATH, from the
/// `bin` of its npm package or the console script of its Python package. The
/// registry names none, and it is not the package name: `@google/gemini-cli`
/// installs `gemini`. A test checks that every such agent has one here.
const REGISTRY_COMMANDS: &[(&str, &str)] = &[
    ("agoragentic-acp", "agoragentic-mcp"),
    ("auggie", "auggie"),
    ("autohand", "autohand-acp"),
    ("claude-acp", "claude-agent-acp"),
    ("cline", "cline"),
    ("codebuddy-code", "codebuddy"),
    ("codex-acp", "codex-acp"),
    ("deepagents", "deepagents-acp"),
    ("dimcode", "dimcode"),
    ("dirac", "dirac"),
    ("factory-droid", "droid"),
    ("fast-agent", "fast-agent-acp"),
    ("gemini", "gemini"),
    ("github-copilot-cli", "copilot"),
    ("glm-acp-agent", "glm-acp-agent"),
    ("grok-build", "grok"),
    ("kilo", "kilo"),
    ("minimax-code", "mcode"),
    ("minion-code", "minion-code"),
    ("nova", "nova"),
    ("pi-acp", "pi-acp"),
    ("qoder", "qodercli"),
    ("qwen-code", "qwen"),
    ("sigit", "sigit"),
];

/// The command that a registry agent installs on the PATH, or none for an agent
/// that the table does not list.
pub(super) fn registry_command(id: &str) -> &'static str {
    REGISTRY_COMMANDS
        .iter()
        .find(|(agent, _)| *agent == id)
        .map_or("", |(_, command)| command)
}

/// A read-only probe that prints `package:<name>` for each package that setup
/// would find installed and reuse: a package in the `.local` prefix or a uv tool
/// of the registry version, or its command that runs on the PATH. It prints no package manager output.
pub(super) fn packages_script(cfg: &Config) -> String {
    let index = index();
    let mut npm = Vec::new();
    let mut uv = Vec::new();
    for agent in index["agents"].as_array().unwrap() {
        let Some(id) = agent["id"].as_str() else {
            continue;
        };
        match install(cfg, id) {
            Install::Npm => npm.extend(
                agent["distribution"]["npx"]["package"]
                    .as_str()
                    .map(|p| quote(&format!("{p}:{}", registry_command(id)))),
            ),
            Install::Uv => uv.extend(
                agent["distribution"]["uvx"]["package"]
                    .as_str()
                    .map(|p| quote(&format!("{p}:{}", registry_command(id)))),
            ),
            Install::Custom | Install::Unsupported => {}
        }
    }
    let providers = ["codex-acp", "claude-acp"]
        .into_iter()
        .filter_map(|id| {
            Some(quote(&format!(
                "{}:{}",
                provider_cli(id)?,
                provider_command(id)?
            )))
        })
        .collect::<Vec<_>>()
        .join(" ");
    // npm prints `path:name@version` lines. setup checks the exact registry version, or
    // any version of a provider CLI, in the same `.local` prefix.
    format!(
        r#"runs() {{ command -v "$1" >/dev/null 2>&1 && "$1" --version </dev/null >/dev/null 2>&1; }}
for pair in {npm} {uvs} {providers}; do
    if runs "${{pair#*:}}"; then printf 'package:%s
' "${{pair%%:*}}"; fi
done
if command -v npm >/dev/null 2>&1; then
    listing="$(npm list --global --prefix "$HOME/.local" --depth=0 --parseable --long 2>/dev/null || true)
"
    for pair in {npm}; do spec=${{pair%%:*}}; case "$listing" in *":$spec
"*) printf 'package:%s
' "$spec";; esac; done
    for pair in {providers}; do name=${{pair%%:*}}; case "$listing" in *":$name@"*) printf 'package:%s
' "$name";; esac; done
fi
if command -v uv >/dev/null 2>&1; then
    tools="
$(uv tool list 2>/dev/null || true)
"
    for pair in {uvs}; do spec=${{pair%%:*}}; case "$tools" in *"
${{spec%%==*}} v${{spec#*==}}
"*) printf 'package:%s
' "$spec";; esac; done
fi
"#,
        npm = npm.join(" "),
        uvs = uv.join(" "),
    )
}

pub(super) fn agent_name(id: &str) -> String {
    index()["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .and_then(|a| a["name"].as_str())
        .unwrap_or(id)
        .to_owned()
}

/// The sign-in an agent uses, as an ID of [`sign_ins`].
pub(super) fn agent_sign_in(id: &str) -> Option<&'static str> {
    match id {
        "codex-acp" => Some("codex"),
        "claude-acp" => Some("claude"),
        "opencode" => Some("opencode"),
        _ => None,
    }
}

/// Every credential reference that a setup request accepts (rule 54).
pub(super) fn sign_ins() -> Vec<SignIn> {
    [
        ("codex", "Codex", true, Some("codex login")),
        ("github", "GitHub", true, Some(GITHUB_LOGIN)),
        ("claude", "Claude", false, Some("claude auth login")),
        ("opencode", "OpenCode", false, Some("opencode auth login")),
        ("ssh", "SSH keys", false, None),
    ]
    .into_iter()
    .map(|(id, name, copy_supported, command)| SignIn {
        id: id.into(),
        name: name.into(),
        copy_supported,
        command: command.map(String::from),
    })
    .collect()
}

fn sign_in(id: &str) -> Option<SignIn> {
    sign_ins().into_iter().find(|s| s.id == id)
}

fn manual(reason: impl Into<String>, command: Option<&str>) -> Option<ManualAction> {
    Some(ManualAction {
        reason: reason.into(),
        command: command.map(String::from),
    })
}

/// The step that does one plan item, or the item itself, in plain words.
pub(super) fn title(id: &str) -> String {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    match (kind, rest) {
        ("probe", _) => "Check the new machine".into(),
        ("worker-capability", _) => "Check that the installed Ariadne can update".into(),
        ("validate", _) => "Check the Ariadne programs".into(),
        ("transfer", _) => "Copy the Ariadne programs".into(),
        ("replacement-capability", _) => "Check that the new Ariadne programs can update".into(),
        ("maintenance", _) => "Wait for running work to finish".into(),
        ("install", _) => "Install the Ariadne programs".into(),
        ("prerequisite", "compiler") => "Check for a C compiler, a linker and the C headers".into(),
        ("prerequisite", "curl") => "Check for curl".into(),
        ("source", _) => "Copy the Ariadne source".into(),
        ("rust", _) => "Install Rust".into(),
        ("build", _) => "Build the Ariadne programs".into(),
        ("prerequisite", "git") => "Check for Git".into(),
        ("tool", "git") => "Install Git".into(),
        ("tool", "github") => "Install GitHub CLI".into(),
        ("tool", agent) => format!("Install {}", agent_name(agent)),
        ("credential", id) => format!(
            "Copy the {} sign-in",
            sign_in(id).map_or(id.to_owned(), |s| s.name)
        ),
        ("authentication", "github") => "Check the GitHub sign-in".into(),
        ("authentication", agent) => format!("Check the {} sign-in", agent_name(agent)),
        ("clone", repository) => format!("Clone repository {repository}"),
        ("repository", repository) => format!("Register repository {repository}"),
        ("service", _) => "Start the Ariadne service".into(),
        ("health", _) => "Check that the Ariadne service answers".into(),
        ("register", _) => "Connect this daemon to the new machine".into(),
        ("resume", _) => "Let the new machine take work again".into(),
        _ => id.into(),
    }
}

/// The work by hand that an item needs when Ariadne cannot do it. `os` is the
/// first part of `uname -s` on the worker, where known. No value depends on
/// command output or on a credential.
pub(super) fn manual_for(cfg: &Config, id: &str, os: Option<&str>) -> Option<ManualAction> {
    let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
    match (kind, rest) {
        ("validate" | "install", _) => manual(
            "The selected Ariadne programs are missing on this machine or do not run on the new machine. Select programs built for the new machine, then review again.",
            None,
        ),
        ("service", _) if os == Some("Darwin") => manual(
            "The new machine has no launchd session for this account. Sign in to this account on the new machine once, then resume setup.",
            None,
        ),
        ("service", _) => manual(
            "The new machine has no systemd user service that keeps running after you sign out. Ask an administrator to turn on lingering for this account, then resume setup.",
            Some("sudo loginctl enable-linger \"$USER\""),
        ),
        ("prerequisite" | "tool", "git") => manual(
            "Git is not on the new machine. Ariadne installs Git only with a user-owned Homebrew, when the request selects tools. Install Git on the new machine, then resume setup.",
            (os == Some("Darwin")).then_some("xcode-select --install"),
        ),
        ("tool", "github") => manual(
            "GitHub CLI is not on the new machine, and Ariadne can install it only with a user-owned Homebrew. Install GitHub CLI on the new machine, then resume setup.",
            None,
        ),
        ("tool", agent) => {
            let name = agent_name(agent);
            match install(cfg, agent) {
                Install::Custom => manual(
                    format!(
                        "This daemon runs {name} with a command from its own configuration. Ariadne cannot install that command. Install it on the new machine."
                    ),
                    None,
                ),
                Install::Npm => manual(
                    format!(
                        "Ariadne installs {name} with npm. The new machine has no Node.js and npm, and no user-owned Homebrew to install them. Install Node.js on the new machine, then resume setup."
                    ),
                    None,
                ),
                Install::Uv => manual(
                    format!(
                        "Ariadne installs {name} with uv. The new machine has no uv, and no user-owned Homebrew to install it. Install uv on the new machine, then resume setup."
                    ),
                    Some("curl -LsSf https://astral.sh/uv/install.sh | sh"),
                ),
                Install::Unsupported => manual(
                    format!("Ariadne cannot install {name}. Install it on the new machine."),
                    None,
                ),
            }
        }
        ("credential", "codex") => manual(
            "This machine has no Codex sign-in file to copy. Sign in to Codex on the new machine.",
            Some("codex login"),
        ),
        ("credential", "github") => manual(
            "GitHub CLI is not signed in on this machine, so there is no sign-in to copy. Sign in to GitHub on the new machine.",
            Some(GITHUB_LOGIN),
        ),
        ("credential", other) => {
            let service = sign_in(other)?;
            manual(
                format!(
                    "Ariadne cannot copy the {} sign-in. Sign in on the new machine.",
                    service.name
                ),
                service.command.as_deref(),
            )
        }
        ("authentication", "github") => manual(
            "GitHub CLI is not signed in on the new machine. Sign in to GitHub on the new machine.",
            Some(GITHUB_LOGIN),
        ),
        ("authentication", "codex-acp") => manual(
            "Codex is not signed in on the new machine. Sign in to Codex on the new machine.",
            Some("codex login"),
        ),
        ("authentication", "claude-acp") => manual(
            "Claude is not signed in on the new machine. Sign in to Claude on the new machine.",
            Some("claude auth login"),
        ),
        ("authentication", agent) => {
            let command = agent_sign_in(agent)
                .and_then(sign_in)
                .and_then(|s| s.command);
            manual(
                format!(
                    "Ariadne cannot check the sign-in of {}. Sign in on the new machine.",
                    agent_name(agent)
                ),
                command.as_deref(),
            )
        }
        _ => None,
    }
}

/// The work by hand for a missing build prerequisite of a source build, with
/// the command for the worker's OS and system package manager, where known.
pub(super) fn build_manual(
    id: &str,
    os: Option<&str>,
    manager: Option<&str>,
) -> Option<ManualAction> {
    let compiler = match id {
        "prerequisite:compiler" => true,
        "prerequisite:curl" => false,
        _ => return None,
    };
    let command = if os == Some("Darwin") {
        compiler.then(|| "xcode-select --install".to_owned())
    } else {
        let (install, tools) = match manager {
            Some("apt-get") => ("sudo apt-get install -y", "build-essential"),
            Some("dnf") => ("sudo dnf install -y", "gcc glibc-devel"),
            Some("yum") => ("sudo yum install -y", "gcc glibc-devel"),
            Some("zypper") => ("sudo zypper install -y", "gcc glibc-devel"),
            Some("pacman") => ("sudo pacman -S --needed --noconfirm", "base-devel"),
            Some("apk") => ("sudo apk add", "build-base"),
            _ => ("", ""),
        };
        (!install.is_empty())
            .then(|| format!("{install} {}", if compiler { tools } else { "curl" }))
    };
    let reason = match (compiler, os) {
        (true, Some("Darwin")) => {
            "The new machine cannot compile and link a C program. The Ariadne build needs a C compiler, a linker and the C library headers. Install the Xcode Command Line Tools on the new machine, then resume setup."
        }
        (true, _) => {
            "The new machine cannot compile and link a C program. The Ariadne build needs a C compiler, a linker and the C library headers. Ask an administrator to install them, then resume setup."
        }
        (false, _) => {
            "The new machine has no Rust and no curl to install Rust with rustup. Ask an administrator to install curl, then resume setup."
        }
    };
    manual(reason, command.as_deref())
}

impl Record {
    /// Progress with a title for each step, and the work by hand for each step
    /// that failed where only the user can continue.
    pub(super) fn view(&self, cfg: &Config) -> SetupDto {
        let mut progress = self.progress.clone();
        let os = self.target.as_deref().map(|t| {
            if t.ends_with("apple-darwin") {
                "Darwin"
            } else {
                "Linux"
            }
        });
        let local = matches!(self.request.source, BinarySource::Local { .. });
        // A source build refuses in `validate` only when the checkout or the commit is missing.
        let build =
            matches!(self.request.source, BinarySource::Daemon) && cfg.build.repository.is_none();
        for step in &mut progress.steps {
            step.title = title(&step.name);
            step.manual = step
                .error
                .as_ref()
                .filter(|_| step.name != "validate" || local)
                .and_then(|_| {
                    manual_for(cfg, &step.name, os)
                        .or_else(|| build_manual(&step.name, os, self.manager.as_deref()))
                });
            if step.name == "validate" && build {
                step.manual = step.error.clone().map(|reason| ManualAction {
                    reason,
                    command: None,
                });
            }
            step.group = Some(if step.skipped {
                SetupGroup::Skipped
            } else if step.manual.is_some() {
                SetupGroup::Manual
            } else if step.complete && self.present.contains(&step.name) {
                SetupGroup::AlreadyPresent
            } else {
                SetupGroup::AriadneInstalls
            });
        }
        progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_npm_or_uv_registry_agent_has_its_executable_named() {
        for agent in index()["agents"].as_array().unwrap() {
            let distribution = &agent["distribution"];
            if distribution["npx"].is_object() || distribution["uvx"].is_object() {
                let id = agent["id"].as_str().unwrap();
                assert!(!registry_command(id).is_empty(), "{id} has no executable");
            }
        }
    }
}
