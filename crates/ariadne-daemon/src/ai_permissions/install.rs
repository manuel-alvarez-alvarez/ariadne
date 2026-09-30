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
use ariadne_api::permissions::{AiPermissionsStatusDto, Device, Flavour};
use ariadne_store::AiPermissionSettingsUpdate;
use tokio::process::Command;

use super::AiPermissions;
use super::flavours::{Pins, pins};

const KEV_COMMIT: &str = "f1535963cea021439370c23127bc970b6788e730";
/// Where PyTorch publishes its CPU-only wheels for Linux. PyPI's Linux wheel
/// carries CUDA, gigabytes that a `cpu` install never uses.
const TORCH_CPU_INDEX: &str = "https://download.pytorch.org/whl/cpu";
/// The PyTorch versions Kev accepts, from its `pyproject.toml`.
const TORCH: &str = "torch>=2.6,<2.9";
/// The file in the venv that names the device its PyTorch was installed for.
const TORCH_DEVICE: &str = "ariadne-torch-device";

/// The `--run` value a flavour serves: `<adapter>@<adapter_revision>`.
pub(crate) fn run_of(flavour: Flavour) -> String {
    let pin = pins(flavour);
    format!("{}@{}", pin.adapter, pin.adapter_revision)
}

/// The release status fields record: `kev@<short commit> <adapter>@<short
/// revision> on <device>`.
fn release(flavour: Flavour, device: Device) -> String {
    let pin = pins(flavour);
    format!(
        "kev@{} {}@{} on {}",
        &KEV_COMMIT[..8],
        pin.adapter,
        &pin.adapter_revision[..8],
        device.as_str()
    )
}

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
            loop {
                let (flavour, device) = ai_permissions.chosen().await;
                let outcome = run(&ai_permissions, flavour, device).await;
                // Skip the deletion where the stored pair already changed. A
                // switch after this check can lose its cache; the reinstall
                // below downloads it again.
                if outcome.is_ok() && ai_permissions.chosen().await == (flavour, device) {
                    remove_other_flavours(&ai_permissions.home, flavour);
                }
                record(&ai_permissions, flavour, device, outcome).await;
                ai_permissions.installing.store(false, Ordering::Release);
                // A switch that landed before the flag cleared found this
                // install busy and only rejoined it, so its pair is installed
                // here. A switch after the read below claims its own install.
                let enabled = ai_permissions
                    .store
                    .ai_permission_settings()
                    .await
                    .is_ok_and(|row| row.enabled);
                if !enabled || ai_permissions.chosen().await == (flavour, device) {
                    break;
                }
                if ai_permissions
                    .installing
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    break;
                }
                ai_permissions
                    .write(AiPermissionSettingsUpdate {
                        state: Some("installing".into()),
                        state_while_enabled: true,
                        last_error: Some(None),
                        ..Default::default()
                    })
                    .await;
            }
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

async fn run(ai_permissions: &AiPermissions, flavour: Flavour, device: Device) -> Result<()> {
    match &ai_permissions.installer {
        Some(command) => stub(ai_permissions, command, flavour, device).await,
        None => {
            let os = ai_permissions.hardware().await.os;
            let venv = venv(ai_permissions).await?;
            package(&venv, &os, device).await?;
            weights(ai_permissions, &venv, pins(flavour)).await
        }
    }
}

async fn record(
    ai_permissions: &AiPermissions,
    flavour: Flavour,
    device: Device,
    outcome: Result<()>,
) {
    let update = match outcome {
        Ok(()) => AiPermissionSettingsUpdate {
            state: Some("ready".into()),
            state_while_enabled: true,
            installed_release: Some(Some(release(flavour, device))),
            latest_release: Some(Some(release(flavour, device))),
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
async fn stub(
    ai_permissions: &AiPermissions,
    command: &[String],
    flavour: Flavour,
    device: Device,
) -> Result<()> {
    let (program, args) = command.split_first().ok_or_else(|| {
        anyhow!("the configured AI permission model installer is an empty command")
    })?;
    let output = Command::new(program)
        .args(args)
        .env("AI_PERMISSIONS_HOME", &ai_permissions.home)
        .env("AI_PERMISSIONS_RUN", run_of(flavour))
        .env("AI_PERMISSIONS_FLAVOUR", flavour.as_str())
        .env("AI_PERMISSIONS_DEVICE", device.as_str())
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

async fn package(venv: &Path, os: &str, device: Device) -> Result<()> {
    let python = venv.join("bin").join("python");
    let marker = venv.join(TORCH_DEVICE);
    let installed_for = std::fs::read_to_string(&marker).ok();
    for args in pip_calls(os, device, installed_for.as_deref().map(str::trim)) {
        run_to_completion(
            Command::new(&python).args(["-m", "pip"]).args(&args),
            "installing the AI permission model package",
        )
        .await?;
    }
    if os == "linux" {
        std::fs::write(&marker, device.as_str())
            .with_context(|| format!("writing {}", marker.display()))?;
    }
    Ok(())
}

/// The `pip` calls of a package install, each the arguments after
/// `python -m pip`. On Linux, PyTorch comes first: from the CPU index for
/// `cpu`, so pip does not download the CUDA build, and from PyPI for `cuda`.
/// A venv whose PyTorch was installed for another device, or for none it
/// recorded, has it uninstalled first, since pip keeps any installed version
/// that meets Kev's range. On macOS the one wheel serves every device.
fn pip_calls(os: &str, device: Device, installed_for: Option<&str>) -> Vec<Vec<String>> {
    let mut calls = Vec::new();
    if os == "linux" {
        if installed_for != Some(device.as_str()) {
            calls.push(vec!["uninstall".into(), "-y".into(), "torch".into()]);
        }
        let mut torch = vec!["install".to_string(), TORCH.to_string()];
        if device == Device::Cpu {
            torch.extend(["--index-url".to_string(), TORCH_CPU_INDEX.to_string()]);
        }
        calls.push(torch);
    }
    calls.push(vec![
        "install".into(),
        "--upgrade".into(),
        format!("kev[serve] @ git+https://github.com/jaredpalmer/kev@{KEV_COMMIT}"),
    ]);
    calls
}

async fn weights(ai_permissions: &AiPermissions, venv: &Path, pin: Pins) -> Result<()> {
    let Pins {
        adapter,
        adapter_revision,
        base,
        base_revision,
    } = pin;
    let script = format!(
        r#"from huggingface_hub import snapshot_download
snapshot_download(repo_id={adapter:?}, revision={adapter_revision:?}, allow_patterns=["*.json", "*.safetensors", "*.pt", "*.txt", "*.jinja"])
snapshot_download(repo_id={base:?}, revision={base_revision:?})
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

/// Delete the Hugging Face cache folders of every flavour but `kept`, its
/// adapter and its base. Called only after an install of `kept` succeeded,
/// so a switch that fails keeps the weights that worked.
fn remove_other_flavours(home: &Path, kept: Flavour) {
    let hub = home.join("hf").join("hub");
    for flavour in Flavour::ALL.into_iter().filter(|&f| f != kept) {
        let pin = pins(flavour);
        for repo in [pin.adapter, pin.base] {
            let folder = hub.join(format!("models--{}", repo.replace('/', "--")));
            if !folder.exists() {
                continue;
            }
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                tracing::warn!(error = %error, folder = %folder.display(), "deleting the weights of another AI permission model flavour failed");
            }
        }
    }
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

    const KEV: &str = "kev[serve] @ git+https://github.com/jaredpalmer/kev@f1535963cea021439370c23127bc970b6788e730";

    fn calls(os: &str, device: Device, installed_for: Option<&str>) -> Vec<String> {
        pip_calls(os, device, installed_for)
            .into_iter()
            .map(|call| call.join(" "))
            .collect()
    }

    /// On Linux, `cpu` takes PyTorch from the CPU index before Kev, and
    /// `cuda` takes it from PyPI; a venv already on the device keeps its
    /// PyTorch, and one on the other device has it uninstalled first.
    #[test]
    fn the_linux_cpu_pip_call_uses_the_cpu_torch_index_and_cuda_does_not() {
        assert_eq!(
            calls("linux", Device::Cpu, Some("cpu")),
            [
                "install torch>=2.6,<2.9 --index-url https://download.pytorch.org/whl/cpu"
                    .to_string(),
                format!("install --upgrade {KEV}"),
            ]
        );
        assert_eq!(
            calls("linux", Device::Cuda, Some("cuda")),
            [
                "install torch>=2.6,<2.9".to_string(),
                format!("install --upgrade {KEV}"),
            ]
        );
        assert_eq!(
            calls("linux", Device::Cpu, Some("cuda")),
            [
                "uninstall -y torch".to_string(),
                "install torch>=2.6,<2.9 --index-url https://download.pytorch.org/whl/cpu"
                    .to_string(),
                format!("install --upgrade {KEV}"),
            ]
        );
        assert_eq!(
            calls("linux", Device::Cuda, None)[..2],
            ["uninstall -y torch", "install torch>=2.6,<2.9"]
        );
    }

    /// On macOS the pip call is the Kev install alone, whatever the device.
    #[test]
    fn the_macos_pip_call_is_kev_alone() {
        for device in [Device::Mlx, Device::Cpu] {
            assert_eq!(
                calls("macos", device, None),
                [format!("install --upgrade {KEV}")]
            );
        }
    }

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
