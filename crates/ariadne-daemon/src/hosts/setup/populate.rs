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
runs() { command -v "$1" >/dev/null 2>&1 && "$1" --version </dev/null >/dev/null 2>&1; }
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
# An update upgrades a program that Homebrew installed, and installs one that does not
# run. It keeps a program that runs and that Homebrew does not own. The program must run
# afterwards, whatever Homebrew reported.
keep_or_upgrade() {
    if runs "$1" && ! { command -v brew >/dev/null 2>&1 && brew list --versions "$1" >/dev/null 2>&1; }; then
        return 0
    fi
    ( upgrade "$1" ) >/dev/null 2>&1 || true
    runs "$1"
}
"#;

/// The functions of an agent script. `runtime` is the one place that decides a
/// runtime is present: its command runs on the PATH. A script prints a
/// `__ARIADNE_MISSING_<what>__` marker and stops when it cannot go on.
const RUNTIME: &str = r#"
missing() { echo "__ARIADNE_MISSING_$1__"; exit 0; }
runtime() {
    if runs "$1"; then return; fi
    out=$(ensure "$1" "$2" 2>/dev/null) || missing "$3"
    case "$out" in *__ARIADNE_INSTALLED__*) echo __ARIADNE_INSTALLED__;; esac
}
upgrade_runtime() {
    [ -z "${upgraded:-}" ] || return 0
    keep_or_upgrade "$1" || missing "$2"
    upgraded=1
}
"#;

/// How long after `Timeouts::worker_runtime_install` the worker ends a runtime install itself,
/// when it never noticed that the SSH connection ended.
const BACKSTOP: std::time::Duration = std::time::Duration::from_secs(120);

/// Printed by a runtime install that failed: the runtime, then the cause.
const RUNTIME_FAILED: &str = "__ARIADNE_RUNTIME_FAILED__ ";

/// Install Node.js or uv into `~/.local` when the worker has no user-owned Homebrew. Setup
/// writes `~/.local/share/ariadne/installed-<command>` for a runtime that it installed, so
/// that an update upgrades it and leaves one that the user installed. `provide` prints
/// `__ARIADNE_INSTALLED__` when it installs, and a `RUNTIME_FAILED` line when it cannot.
const LOCAL_RUNTIMES: &str = r#"
fetch() { curl --proto '=https' --tlsv1.2 -fsSL -o "$2" "$1" </dev/null >/dev/null 2>&1; }
sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
mark() { mkdir -p "$HOME/.local/share/ariadne" && printf '%s\n' "$1" > "$HOME/.local/share/ariadne/installed-$2"; }
node_archive() {
    dir=$1
    case $(uname -s) in Linux) os=linux;; Darwin) os=darwin;; *) echo "there is no Node.js build for $(uname -s)"; return 1;; esac
    case $(uname -m) in x86_64|amd64) arch=x64;; aarch64|arm64) arch=arm64;; *) echo "there is no Node.js build for $(uname -m)"; return 1;; esac
    base=https://nodejs.org/dist
    fetch "$base/index.json" "$dir/index.json" || { echo "the download of $base/index.json failed"; return 1; }
    version=$(grep -m1 '"lts":"' "$dir/index.json" | sed -n 's/.*"version":"\(v[0-9.]*\)".*/\1/p')
    [ -n "$version" ] || { echo "$base/index.json lists no LTS release"; return 1; }
    file=node-$version-$os-$arch.tar.gz
    fetch "$base/$version/SHASUMS256.txt" "$dir/SHASUMS256.txt" || { echo "the download of $base/$version/SHASUMS256.txt failed"; return 1; }
    fetch "$base/$version/$file" "$dir/$file" || { echo "the download of $base/$version/$file failed"; return 1; }
    want=$(grep "  $file\$" "$dir/SHASUMS256.txt" | cut -d' ' -f1)
    have=$(sha256 "$dir/$file")
    [ -n "$want" ] && [ "$want" = "$have" ] || { echo "the checksum of $file does not match SHASUMS256.txt of $version"; return 1; }
    mkdir -p "$HOME/.local"
    tar -xzf "$dir/$file" -C "$HOME/.local" --strip-components=1 --exclude='*/LICENSE' --exclude='*/README.md' --exclude='*/CHANGELOG.md' >/dev/null 2>&1 || { echo "the archive $file cannot be extracted into ~/.local"; return 1; }
    mark "$version" node
}
uv_installer() {
    dir=$1
    fetch https://astral.sh/uv/install.sh "$dir/install.sh" || { echo "the download of https://astral.sh/uv/install.sh failed"; return 1; }
    mkdir -p "$HOME/.local/bin"
    UV_UNMANAGED_INSTALL="$HOME/.local/bin" sh "$dir/install.sh" >/dev/null 2>&1 || { echo "the uv installer failed"; return 1; }
    mark standalone uv
}
install_local() {
    command -v curl >/dev/null 2>&1 || { echo "curl is missing on the worker"; return 1; }
    dir=$(mktemp -d "${TMPDIR:-/tmp}/ariadne-runtime.XXXXXX") || { echo "cannot make a temporary directory"; return 1; }
    if cause=$("$1" "$dir"); then rm -rf "$dir"; else rm -rf "$dir"; echo "$cause"; return 1; fi
}
present() { if [ "$1" = node ]; then runs node && runs npm; else runs "$1"; fi; }
homebrew() {
    command -v brew >/dev/null 2>&1 && [ "$(id -u)" != 0 ] || return 1
    prefix=$(brew --prefix 2>/dev/null) && [ -O "$prefix" ] && [ -w "$prefix" ]
}
# A present runtime stays, and so does a user-owned Homebrew's job. An update upgrades one
# that setup installed under ~/.local.
provide() {
    if [ -n "$update" ] && [ -f "$HOME/.local/share/ariadne/installed-$1" ]; then :
    elif present "$1" || homebrew; then return 0
    fi
    case $1 in node) installer=node_archive;; *) installer=uv_installer;; esac
    if cause=$(install_local "$installer") && present "$1"; then echo __ARIADNE_INSTALLED__
    else echo "__ARIADNE_RUNTIME_FAILED__ $1: ${cause:-the program does not run after the install}"; return 3; fi
}
# Run `provide` in a process group of its own. End that group when the SSH connection ends,
# which closes the stdin of this script, and after `limit` seconds in case that is never
# noticed. No download or install outlives its step on the worker. A failure stops the
# script; its line is already printed.
bounded() {
    set -m
    "$@" & work=$!
    ( cat >/dev/null 2>&1; kill -TERM -- "-$work" 2>/dev/null ) & gone=$!
    ( sleep "$limit"; kill -TERM -- "-$work" 2>/dev/null ) & dog=$!
    rc=0
    wait "$work" || rc=$?
    kill -TERM -- "-$gone" "-$dog" 2>/dev/null || true
    wait "$gone" "$dog" 2>/dev/null || true
    [ "$rc" != 0 ] || return 0
    [ "$rc" = 3 ] || echo "__ARIADNE_RUNTIME_FAILED__ $2: ended after ${limit}s"
    exit 0
}
"#;

const MISSING_NODE: &str = "__ARIADNE_MISSING_NODE__";
const MISSING_UV: &str = "__ARIADNE_MISSING_UV__";
const MISSING_PACKAGE: &str = "__ARIADNE_MISSING_PACKAGE__";
const MISSING_PROVIDER: &str = "__ARIADNE_MISSING_PROVIDER__";

/// What a finished agent script reports as missing, in words for the user.
fn missing_error(output: &str, id: &str) -> Option<String> {
    let provider = super::plan::provider_cli(id).unwrap_or("the provider CLI");
    if output.contains(MISSING_NODE) {
        Some("Node.js and npm are missing on the worker; install them, or provide user-owned Homebrew, and resume setup".into())
    } else if output.contains(MISSING_UV) {
        Some("uv is missing on the worker; install uv, or provide user-owned Homebrew, and resume setup".into())
    } else if output.contains(MISSING_PACKAGE) {
        Some(format!(
            "the ACP package of {id} is missing on the worker and its installation failed; install it and resume setup"
        ))
    } else if output.contains(MISSING_PROVIDER) {
        Some(format!(
            "the provider CLI {provider} is missing on the worker and its installation failed; install it and resume setup"
        ))
    } else {
        None
    }
}

/// Install one npm package into `~/.local` unless its `command` runs on the PATH or
/// the package is there. An update leaves a program that the user installed by hand alone.
fn npm_package(package: &str, command: &str, update: bool, failure: &str) -> String {
    let spec = quote(package);
    let name = quote(super::plan::package_name(package));
    let command = quote(command);
    let list = "npm list --global --prefix \"$HOME/.local\" --depth=0";
    let check = format!("{list} {spec} >/dev/null 2>&1");
    let install =
        format!("npm install --global --prefix \"$HOME/.local\" -- {spec} >/dev/null 2>&1");
    let found = format!("runs {command}");
    if update {
        format!(
            "if ! {found} || {{ command -v npm >/dev/null 2>&1 && {list} {name} >/dev/null 2>&1; }}; then\nupgrade_runtime node NODE\nruntime npm node NODE\n{install} || missing {failure}\n{check} || missing {failure}\nfi"
        )
    } else {
        format!(
            "if {found} || {check}; then :; else\nruntime node node NODE\nruntime npm node NODE\n{install} || missing {failure}\n{check} || missing {failure}\necho {INSTALLED}\nfi"
        )
    }
}

/// The shell test that is true when `npm_package` runs its install.
fn npm_needed(package: &str, command: &str, update: bool) -> String {
    let command = quote(command);
    if update {
        let name = quote(super::plan::package_name(package));
        format!(
            "! runs {command} || {{ command -v npm >/dev/null 2>&1 && npm list --global --prefix \"$HOME/.local\" --depth=0 {name} >/dev/null 2>&1; }}"
        )
    } else {
        let spec = quote(package);
        format!(
            "! {{ runs {command} || npm list --global --prefix \"$HOME/.local\" --depth=0 {spec} >/dev/null 2>&1; }}"
        )
    }
}

/// The shell test that is true when the uv tool `package` (`name==version`) runs its install.
fn uv_needed(package: &str, command: &str, update: bool) -> String {
    let (tool, version) = package.split_once("==").unwrap_or((package, ""));
    let command = quote(command);
    if update {
        format!(
            "! runs {command} || {{ command -v uv >/dev/null 2>&1 && uv tool list 2>/dev/null | grep -q \"^{tool} \"; }}"
        )
    } else {
        format!(
            "! {{ runs {command} || uv tool list 2>/dev/null | grep -Fqx -- {}; }}",
            quote(&format!("{tool} v{version}"))
        )
    }
}

/// What a runtime install that failed reports, in words for the user.
fn runtime_error(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find_map(|l| l.strip_prefix(RUNTIME_FAILED))?;
    let (runtime, cause) = line.split_once(": ").unwrap_or(("", line));
    Some(match runtime {
        "uv" => format!(
            "uv is missing on the worker and its installation in ~/.local failed: {cause}; install uv, or provide user-owned Homebrew, and resume setup"
        ),
        _ => format!(
            "Node.js and npm are missing on the worker and their installation in ~/.local failed: {cause}; install them, or provide user-owned Homebrew, and resume setup"
        ),
    })
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

    /// Install a missing Node.js or uv into the worker user's home, where `needs` says an
    /// agent script would use it and the worker has no user-owned Homebrew. Returns what the
    /// script printed.
    async fn install_runtimes(
        &self,
        request: &SetupRequest,
        path: &str,
        update: bool,
        needs: &[(String, &str)],
    ) -> Result<String, String> {
        let limit = self.0.timeouts.worker_runtime_install;
        let mut script = format!(
            "{PREREQUISITE}\n{LOCAL_RUNTIMES}\nupdate={}\nlimit={}\n",
            if update { "1" } else { "" },
            (limit + BACKSTOP).as_secs_f64().ceil()
        );
        for (condition, runtime) in needs {
            script.push_str(&format!(
                "if {condition}; then bounded provide {runtime}; fi\n"
            ));
        }
        let output = self
            .ssh_held(
                request,
                &format!("set -eu; umask 077; export PATH={}; {script}", quote(path)),
                None,
                limit,
                true,
            )
            .await
            .map_err(|e| {
                if e.contains("timed out") {
                    "the download or install of Node.js or uv timed out; install the runtime by hand and resume setup".to_owned()
                } else {
                    "the install of Node.js or uv failed on the worker; install the runtime by hand and resume setup".to_owned()
                }
            })?;
        match runtime_error(&output) {
            Some(error) => Err(error),
            None => Ok(output),
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
                        format!("keep_or_upgrade {formula}")
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
                    let mut needs: Vec<(String, &str)> = Vec::new();
                    let script = if let Some(package) = distribution["npx"]["package"].as_str() {
                        needs.push((npm_needed(package, super::plan::registry_command(id), update), "node"));
                        format!(
                            "{PREREQUISITE}\n{RUNTIME}\nexport npm_config_cache=\"$HOME/.cache/ariadne/npm\"\n{}",
                            npm_package(package, super::plan::registry_command(id), update, "PACKAGE")
                        )
                    } else if let Some(package) = distribution["uvx"]["package"].as_str() {
                        let (tool, version) = package.split_once("==").unwrap_or((package, ""));
                        needs.push((uv_needed(package, super::plan::registry_command(id), update), "uv"));
                        let command = quote(super::plan::registry_command(id));
                        let package = quote(package);
                        let body = if update {
                            format!("if ! runs {command} || {{ command -v uv >/dev/null 2>&1 && uv tool list 2>/dev/null | grep -q \"^{tool} \"; }}; then\nupgrade_runtime uv UV\nexport UV_TOOL_BIN_DIR=\"$HOME/.local/bin\"\nuv python install >/dev/null 2>&1 || missing PACKAGE\nuv tool install --upgrade -- {package} >/dev/null 2>&1 || uv tool upgrade -- {package} >/dev/null 2>&1 || missing PACKAGE\nfi")
                        } else {
                            format!("if runs {command} || uv tool list 2>/dev/null | grep -Fqx -- {listed}; then :; else\nruntime uv uv UV\nexport UV_TOOL_BIN_DIR=\"$HOME/.local/bin\"\nuv python install >/dev/null 2>&1 || missing PACKAGE\nuv tool install -- {package} >/dev/null 2>&1 || uv tool upgrade -- {package} >/dev/null 2>&1 || missing PACKAGE\necho {INSTALLED}\nfi", listed = quote(&format!("{tool} v{version}")))
                        };
                        format!("{PREREQUISITE}\n{RUNTIME}\n{body}")
                    } else {
                        return Err("unsupported distribution; manual installation and worker login are required".into());
                    };
                    let script = match super::plan::provider_cli(id) {
                        Some(cli) => {
                            needs.push((npm_needed(cli, super::plan::provider_command(id).unwrap(), update), "node"));
                            format!(
                            "{script}\n{}",
                            npm_package(cli, super::plan::provider_command(id).unwrap(), update, "PROVIDER")
                            )
                        }
                        None => script,
                    };
                    let installed = self.install_runtimes(request, path, update, &needs).await?;
                    let output = self.population_command(request, path, &script, None).await.map_err(|_| "agent installation or runtime check failed; provide user-owned Homebrew or ask an administrator for Node/npm or uv prerequisites".to_owned())?;
                    if let Some(error) = missing_error(&output, id) {
                        return Err(error);
                    }
                    let installed = installed.contains(INSTALLED) || output.contains(INSTALLED);
                    self.found_present(name, &step, !update && !installed);
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
