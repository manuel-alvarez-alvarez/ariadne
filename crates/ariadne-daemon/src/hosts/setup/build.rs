//! Build this daemon's commit on the worker, for a daemon that is a source
//! build. Setup sends the commit only, installs Rust in the worker user's home
//! when it is missing, and runs no administrator command.
use super::populate::INSTALLED;
use super::*;

/// A probe that changes no worker file. It needs `os`, the worker's `uname -s`,
/// and prints `manager:<name>` for the system package manager, then
/// `compiler`, `cargo`, `rustup` and `curl` for each build prerequisite that
/// works. `compiler` compiles and links a small C program with the standard C
/// headers, in a temporary directory that it removes. `cargo` and `rustup`
/// use the PATH of the build, with `~/.cargo/bin` first.
pub(super) const BUILD_PROBE: &str = r#"for manager in apt-get dnf yum zypper pacman apk; do
    if command -v "$manager" >/dev/null 2>&1; then printf 'manager:%s\n' "$manager"; break; fi
done
ariadne_compiles() {
    { [ "$os" != Darwin ] || xcode-select -p >/dev/null 2>&1; } || return 1
    command -v cc >/dev/null 2>&1 || return 1
    probe=$(mktemp -d "${TMPDIR:-/tmp}/ariadne-probe.XXXXXX") || return 1
    printf '#include <math.h>\n#include <pthread.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\nint main(void) { return 0; }\n' > "$probe/probe.c"
    if cc "$probe/probe.c" -o "$probe/probe" -lpthread -lm >/dev/null 2>&1; then compiled=0; else compiled=1; fi
    rm -rf "$probe"
    return "$compiled"
}
if ariadne_compiles; then echo compiler; fi
if (PATH="$HOME/.cargo/bin:$PATH"; cargo --version) >/dev/null 2>&1; then echo cargo; fi
if (PATH="$HOME/.cargo/bin:$PATH"; command -v rustup) >/dev/null 2>&1; then echo rustup; fi
if command -v curl >/dev/null 2>&1; then echo curl; fi
"#;

/// Reuse a Cargo that runs. A rustup without a toolchain installs one, and
/// only a worker without rustup downloads the rustup installer with curl.
const RUST: &str = r#"if cargo --version >/dev/null 2>&1; then exit 0; fi
if command -v rustup >/dev/null 2>&1; then
    rustup toolchain install stable --profile minimal >/dev/null 2>&1
    rustup default stable >/dev/null 2>&1
else
    command -v curl >/dev/null 2>&1
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal >/dev/null 2>&1
fi
echo __ARIADNE_INSTALLED__
# The shell remembers where it found the Cargo that did not run.
hash -r
cargo --version >/dev/null 2>&1
"#;

impl Hosts {
    /// Whether this operation builds the daemon's commit on the worker.
    pub(super) fn source_build(&self, request: &SetupRequest, binaries: bool) -> bool {
        binaries
            && matches!(request.source, BinarySource::Daemon)
            && self.0.cfg.build.repository.is_none()
    }

    /// Run a build command with the worker user's PATH and Rust in the home.
    async fn build_command(
        &self,
        request: &SetupRequest,
        path: &str,
        script: &str,
        input: Option<Vec<u8>>,
    ) -> Result<String, String> {
        self.ssh_within(
            request,
            &format!(
                "set -eu; umask 077; export PATH=\"$HOME/.cargo/bin\":{}; {script}",
                quote(path)
            ),
            input,
            self.0.timeouts.worker_source_build,
        )
        .await
    }

    /// Check the C compiler, the linker, the C headers and curl. A missing one is manual
    /// work: setup records it on its step and returns `false`.
    pub(super) async fn build_prerequisites(
        &self,
        name: &str,
        request: &SetupRequest,
        os: &str,
        path: &str,
    ) -> Result<bool, String> {
        let update = name.starts_with("update:");
        self.begin(name, "prerequisite:compiler")?;
        let output = self
            .population_command(
                request,
                path,
                &format!("os={}\n{BUILD_PROBE}", quote(os)),
                None,
            )
            .await
            .map_err(|_| "cannot check the build prerequisites on the worker".to_owned())?;
        let found: Vec<&str> = output.lines().collect();
        self.0.setup.lock().unwrap().get_mut(name).unwrap().manager = found
            .iter()
            .find_map(|line| line.strip_prefix("manager:"))
            .map(String::from);
        let mut ready = true;
        for (step, met, error) in [
            (
                "prerequisite:compiler",
                found.contains(&"compiler"),
                "the worker cannot compile and link a C program; ask an administrator to install a C compiler, a linker and the C library headers, then resume setup",
            ),
            (
                "prerequisite:curl",
                ["cargo", "rustup", "curl"]
                    .iter()
                    .any(|tool| found.contains(tool)),
                "curl is missing on the worker and Rust is not installed; ask an administrator to install curl, then resume setup",
            ),
        ] {
            self.begin(name, step)?;
            if met {
                self.found_present(name, step, !update);
                self.checkpoint(name, step)?;
            } else {
                self.population_result(name, step, Err(error.into()))?;
                ready = false;
            }
        }
        Ok(ready)
    }

    /// Send the commit, install Rust when it is missing, build both programs
    /// in release mode, and stage them where `install` takes them from.
    pub(super) async fn build_programs(
        &self,
        name: &str,
        request: &SetupRequest,
        path: &str,
        stage: &str,
    ) -> Result<(), String> {
        let update = name.starts_with("update:");
        let build = &self.0.cfg.build;
        let commit = quote(&build.commit);
        self.begin(name, "source")?;
        let present = self
            .population_command(
                request,
                path,
                &format!(
                    r#"[ "$(cat "$HOME/.ariadne/worker-source/.ariadne-commit" 2>/dev/null)" = {commit} ]"#
                ),
                None,
            )
            .await
            .is_ok();
        if !present {
            let dir = self.0.cfg.root.join("worker-source");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let archive = dir.join(format!("{name}.tar.gz"));
            // `git archive` packs the committed tree only: no uncommitted edit
            // and no untracked file leaves this machine.
            let packed = self
                .command(
                    Command::new("git")
                        .arg("-C")
                        .arg(&build.checkout)
                        .args(["archive", "--format=tar.gz", "-o"])
                        .arg(&archive)
                        .arg(&build.commit),
                    None,
                )
                .await
                .and_then(|_| std::fs::read(&archive).map_err(|e| e.to_string()));
            let _ = std::fs::remove_file(&archive);
            let data = packed.map_err(|_| {
                format!(
                    "cannot pack commit {} from the checkout {}",
                    build.commit, build.checkout
                )
            })?;
            self.build_command(
                request,
                path,
                &format!(
                    r#"src="$HOME/.ariadne/worker-source"
rm -rf "$src.partial"
mkdir -p "$src.partial"
tar -xzf - -C "$src.partial"
printf '%s\n' {commit} > "$src.partial/.ariadne-commit"
rm -rf "$src"
mv "$src.partial" "$src""#
                ),
                Some(data),
            )
            .await
            .map_err(|_| {
                "the source transfer to the worker failed; check SSH access and free disk space, then retry"
                    .to_owned()
            })?;
        }
        self.found_present(name, "source", !update && present);
        self.checkpoint(name, "source")?;
        self.begin(name, "rust")?;
        let output = self
            .build_command(request, path, RUST, None)
            .await
            .map_err(|_| {
                "Rust installation with rustup failed on the worker; check its access to sh.rustup.rs and static.rust-lang.org, then retry"
                    .to_owned()
            })?;
        self.found_present(name, "rust", !update && !output.contains(INSTALLED));
        self.checkpoint(name, "rust")?;
        self.begin(name, "build")?;
        // A completed build of the same commit is reused, not built again.
        let output = self
            .build_command(
                request,
                path,
                &format!(
                    r#"target="$HOME/.ariadne/worker-build"
if [ "$(cat "$target/ariadne-built-commit" 2>/dev/null)" != {commit} ] || [ ! -x "$target/release/ariadne" ] || [ ! -x "$target/release/ariadned" ]; then
    mkdir -p "$target"
    rm -f "$target/ariadne-built-commit"
    cd "$HOME/.ariadne/worker-source"
    ARIADNE_RELEASE_REPOSITORY= ARIADNE_SOURCE_COMMIT={commit} CARGO_TARGET_DIR="$target" cargo build --release --locked --bin ariadne --bin ariadned >"$target/build.log" 2>&1
    printf '%s\n' {commit} > "$target/ariadne-built-commit"
    echo {INSTALLED}
fi
mkdir -p {stage}
cp "$target/release/ariadne" "$target/release/ariadned" {stage}/"#,
                    stage = quote(stage)
                ),
                None,
            )
            .await
            .map_err(|_| {
                "the release build failed on the worker; read ~/.ariadne/worker-build/build.log on the worker, then retry"
                    .to_owned()
            })?;
        self.found_present(name, "build", !update && !output.contains(INSTALLED));
        self.checkpoint(name, "build")
    }
}
