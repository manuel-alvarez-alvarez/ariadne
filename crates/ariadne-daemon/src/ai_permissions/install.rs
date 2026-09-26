//! Installing the model: its virtual environment, pinned package and weights.
//!
//! One install runs at a time, as a background tokio task, because it takes
//! minutes and downloads gigabytes. Nothing waits on it: the write that
//! started it answers `installing`, and every state it reaches afterwards is
//! published as `ai_permissions_updated`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result, anyhow, bail};
use ariadne_api::permissions::AiPermissionsStatusDto;
use ariadne_store::AiPermissionSettingsUpdate;
use tokio::process::Command;

use super::{AiPermissions, decide};

const KEV_COMMIT: &str = "f1535963cea021439370c23127bc970b6788e730";
const ADAPTER: &str = "jaredpalmer/kev-4b";
const ADAPTER_REVISION: &str = "139fdd94f1b6a6ad80cc15e08fcb99cac885a101";
const BASE: &str = "Qwen/Qwen3.5-4B-Base";
const BASE_REVISION: &str = "1001bb4d826a52d1f399e183466143f4da7b741b";
/// The pin status fields record: `kev@<short commit> <run>`.
static PIN: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("kev@{} {ADAPTER}@{ADAPTER_REVISION}", &KEV_COMMIT[..8],));

impl AiPermissions {
    /// Start an install in the background, and answer the status it leaves
    /// behind — `installing`, written before this returns, so the caller
    /// hands the client a status the install cannot have moved past yet.
    pub(crate) async fn install(&self) -> Option<AiPermissionsStatusDto> {
        if self
            .installing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        let status = self
            .write(AiPermissionSettingsUpdate {
                state: Some("installing".into()),
                state_while_enabled: true,
                last_error: Some(None),
                ..Default::default()
            })
            .await;
        let ai_permissions = self.clone();
        tokio::spawn(async move {
            let outcome = run(&ai_permissions).await;
            record(&ai_permissions, outcome).await;
            ai_permissions.installing.store(false, Ordering::Release);
        });
        Some(status)
    }

    /// Say again that the install running now is the one the model waits for.
    pub async fn rejoin_install(&self) -> AiPermissionsStatusDto {
        self.write(AiPermissionSettingsUpdate {
            state: Some("installing".into()),
            state_while_enabled: true,
            ..Default::default()
        })
        .await
    }

    /// Write `update` and publish the status it leaves behind.
    pub(crate) async fn write(&self, update: AiPermissionSettingsUpdate) -> AiPermissionsStatusDto {
        if let Err(error) = self.store.update_ai_permission_settings(update).await {
            tracing::warn!(error = %error, "writing the AI permission settings failed");
        }
        let status = self.announce().await;
        self.notify_server();
        status
    }
}

async fn run(ai_permissions: &AiPermissions) -> Result<()> {
    match &ai_permissions.installer {
        Some(command) => stub(ai_permissions, command).await,
        None => {
            let venv = venv(ai_permissions).await?;
            package(&venv).await?;
            weights(ai_permissions, &venv).await
        }
    }
}

async fn record(ai_permissions: &AiPermissions, outcome: Result<()>) {
    let update = match outcome {
        Ok(()) => AiPermissionSettingsUpdate {
            state: Some("ready".into()),
            state_while_enabled: true,
            installed_release: Some(Some(PIN.to_string())),
            latest_release: Some(Some(PIN.to_string())),
            weights_present: Some(true),
            last_refresh_at: Some(Some(
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            )),
            last_error: Some(None),
            ..Default::default()
        },
        Err(error) => {
            let reason = format!("{error:#}");
            tracing::warn!(error = %reason, "installing the AI permission model failed");
            AiPermissionSettingsUpdate {
                state: Some("failed".into()),
                state_while_enabled: true,
                last_error: Some(Some(reason)),
                ..Default::default()
            }
        }
    };
    ai_permissions.write(update).await;
}

/// The command that stands in for the virtual environment, package and weights in the suite.
async fn stub(ai_permissions: &AiPermissions, command: &[String]) -> Result<()> {
    let (program, args) = command.split_first().ok_or_else(|| {
        anyhow!("the configured AI permission model installer is an empty command")
    })?;
    let output = Command::new(program)
        .args(args)
        .env("AI_PERMISSIONS_HOME", &ai_permissions.home)
        .env("AI_PERMISSIONS_RUN", decide::RUN)
        .env("AI_PERMISSIONS_KEV_COMMIT", KEV_COMMIT)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| {
            format!(
                "running the AI permission model installer `{}`",
                command.join(" ")
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match said.is_empty() {
        true => bail!(
            "the AI permission model installer {}",
            ended(&output.status)
        ),
        false => bail!("{said}"),
    }
}

/// Create a virtual environment with a supported interpreter. A venv made by
/// an unsupported interpreter is rebuilt, while its Hugging Face cache stays.
async fn venv(ai_permissions: &AiPermissions) -> Result<PathBuf> {
    let venv = ai_permissions.home.join("venv");
    let interpreter = venv.join("bin").join("python");
    if interpreter.is_file() && venv_interpreter_is_supported(&interpreter).await {
        return Ok(venv);
    }
    if venv.exists() {
        std::fs::remove_dir_all(&venv).with_context(|| format!("rebuilding {}", venv.display()))?;
    }
    let python = super::python::probe_python(
        ai_permissions.python_bin.as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .await;
    let (Some(path), true) = (python.path.as_deref(), python.ok) else {
        bail!(
            "the AI permission model needs Python 3.12 or 3.13; this daemon found {}",
            python.version.as_deref().unwrap_or("none"),
        );
    };
    std::fs::create_dir_all(&ai_permissions.home)
        .with_context(|| format!("creating {}", ai_permissions.home.display()))?;
    run_to_completion(
        Command::new(path).args(["-m", "venv"]).arg(&venv),
        "creating the AI permission model virtual environment",
    )
    .await?;
    Ok(venv)
}

async fn venv_interpreter_is_supported(interpreter: &Path) -> bool {
    super::python::probe_python(Some(&interpreter.display().to_string()), None)
        .await
        .ok
}

async fn package(venv: &Path) -> Result<()> {
    run_to_completion(
        Command::new(venv.join("bin").join("python")).args([
            "-m",
            "pip",
            "install",
            "--upgrade",
            &format!("kev[serve] @ git+https://github.com/jaredpalmer/kev@{KEV_COMMIT}"),
        ]),
        "installing the AI permission model package",
    )
    .await
}

async fn weights(ai_permissions: &AiPermissions, venv: &Path) -> Result<()> {
    let script = format!(
        r#"from huggingface_hub import snapshot_download
snapshot_download(repo_id={ADAPTER:?}, revision={ADAPTER_REVISION:?}, allow_patterns=["*.json", "*.safetensors", "*.pt", "*.txt", "*.jinja"])
snapshot_download(repo_id={BASE:?}, revision={BASE_REVISION:?})
"#,
    );
    run_to_completion(
        Command::new(venv.join("bin").join("python"))
            .args(["-c", &script])
            .env("HF_HOME", ai_permissions.home.join("hf")),
        "downloading the AI permission model adapter and base",
    )
    .await
}

async fn run_to_completion(command: &mut Command, what: &'static str) -> Result<()> {
    let output = command
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| what.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match said.is_empty() {
        true => bail!("{what} {}", ended(&output.status)),
        false => bail!("{what}: {said}"),
    }
}

fn ended(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "was killed by a signal".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn a_venv_with_a_supported_interpreter_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let supported = dir.path().join("python-3.13");
        let unsupported = dir.path().join("python-3.14");
        for (path, version) in [(&supported, "3.13.4"), (&unsupported, "3.14.0")] {
            std::fs::write(path, format!("#!/bin/sh\necho 'Python {version}'\n")).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(venv_interpreter_is_supported(&supported).await);
        assert!(!venv_interpreter_is_supported(&unsupported).await);
    }
}
