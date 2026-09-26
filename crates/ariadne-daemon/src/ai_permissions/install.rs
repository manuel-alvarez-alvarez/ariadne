//! Installing the model: the release document, the virtual environment, the wheel
//! and the weights.
//!
//! One install runs at a time, as a background tokio task, because it takes
//! minutes and downloads gigabytes — PyTorch, and 843 MB of English weights
//! or 2.4 GB of all three checkpoints. Nothing waits on it: the write that
//! started it answers `installing`, and every state it reaches afterwards is
//! published as `ai_permissions_updated`.
//!
//! An install that fails leaves the one before it on disk. A half-finished
//! download is the reason: the files of a model that worked yesterday are
//! better than none, and `state = failed` with `last_error` says what to fix.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use ariadne_api::permissions::{AiPermissionsCheckpoints, AiPermissionsStatusDto};
use ariadne_store::AiPermissionSettingsUpdate;
use tokio::process::Command;

use super::{AiPermissions, checkpoints_of};

/// What the release document names: the tag the install records, and the
/// wheel it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Release {
    tag: String,
    wheel_url: String,
}

/// The checkpoints each choice downloads, as
/// `from laya import Router; Router().preload([…])` names them.
const ENGLISH: [&str; 1] = ["english"];
const ALL: [&str; 3] = ["english", "multilingual", "typed-decisions"];

impl AiPermissions {
    /// Start an install in the background, and answer the status it leaves
    /// behind — `installing`, written before this returns, so the caller
    /// hands the client a status the install cannot have moved past yet.
    ///
    /// `None` where an install is already running: one at a time, whatever
    /// asks.
    ///
    /// The model can be turned off while it runs. Every state the install writes
    /// lands only on a row that is still enabled, so the install ends as
    /// whatever it ends as — its release and its error are still written —
    /// and the model stays off.
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

    /// Say again that the install running now is the one the model waits for:
    /// what turning the model on answers while an earlier install still runs. That
    /// install's end writes `ready` or `failed` over this.
    ///
    /// Guarded like the install's own writes. A turn-off can land after the
    /// write that turned the model on and before this one; it keeps its
    /// `disabled`, and nothing leaves a model that is off at `installing`.
    pub async fn rejoin_install(&self) -> AiPermissionsStatusDto {
        self.write(AiPermissionSettingsUpdate {
            state: Some("installing".into()),
            state_while_enabled: true,
            ..Default::default()
        })
        .await
    }

    /// Write `update` and publish the status it leaves behind. A write that
    /// fails is logged and the status is still published: a client left with
    /// no event at all would wait for one that never comes.
    pub(crate) async fn write(&self, update: AiPermissionSettingsUpdate) -> AiPermissionsStatusDto {
        if let Err(error) = self.store.update_ai_permission_settings(update).await {
            tracing::warn!(error = %error, "writing the AI permission settings failed");
        }
        let status = self.announce().await;
        self.notify_server();
        status
    }
}

/// The whole install, start to finish. Every failure is an error carrying the
/// sentence `last_error` ends up with.
async fn run(ai_permissions: &AiPermissions) -> Result<Release> {
    let release = release(
        &ai_permissions.release_url,
        ai_permissions.timeouts.ai_permissions_release_download,
    )
    .await?;
    let row = ai_permissions
        .store
        .ai_permission_settings()
        .await
        .context("reading the AI permission settings")?;
    let checkpoints = checkpoints_of(&row.checkpoints);
    match &ai_permissions.installer {
        Some(command) => stub(ai_permissions, command, &release, checkpoints).await?,
        None => {
            let venv = venv(ai_permissions).await?;
            wheel(&venv, &release).await?;
            weights(ai_permissions, &venv, checkpoints).await?;
        }
    }
    Ok(release)
}

/// What the install ended as: `ready` with the release it put on disk, or
/// `failed` with why — and, either way, an event saying so.
async fn record(ai_permissions: &AiPermissions, outcome: Result<Release>) {
    let update = match outcome {
        Ok(release) => AiPermissionSettingsUpdate {
            state: Some("ready".into()),
            state_while_enabled: true,
            installed_release: Some(Some(release.tag.clone())),
            latest_release: Some(Some(release.tag)),
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

/// The release document, read for the tag and the one `.whl` asset on it.
async fn release(url: &str, timeout: Duration) -> Result<Release> {
    let body = reqwest::Client::builder()
        .timeout(timeout)
        // GitHub's API refuses a request that does not name its caller.
        .user_agent(concat!("ariadned/", env!("CARGO_PKG_VERSION")))
        .build()?
        .get(url)
        .send()
        .await
        .with_context(|| {
            format!("downloading the AI permission model release document from {url}")
        })?
        .error_for_status()
        .with_context(|| {
            format!("downloading the AI permission model release document from {url}")
        })?
        .bytes()
        .await
        .with_context(|| {
            format!("downloading the AI permission model release document from {url}")
        })?;
    let document: serde_json::Value = serde_json::from_slice(&body)
        .context("reading the AI permission model release document")?;

    let tag = document["tag_name"]
        .as_str()
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| anyhow!("the AI permission model release document names no tag_name"))?;
    let assets = document["assets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let wheel_url = assets
        .iter()
        .filter_map(|asset| asset["browser_download_url"].as_str())
        .find(|url| url.ends_with(".whl"))
        .ok_or_else(|| anyhow!("the AI permission model release {tag} carries no .whl asset"))?;
    Ok(Release {
        tag: tag.to_string(),
        wheel_url: wheel_url.to_string(),
    })
}

/// The command that stands in for the venv, the wheel and the weights, in the
/// suite. Its exit status decides the install, and its stderr says why one
/// that failed did.
async fn stub(
    ai_permissions: &AiPermissions,
    command: &[String],
    release: &Release,
    checkpoints: AiPermissionsCheckpoints,
) -> Result<()> {
    let (program, args) = command.split_first().ok_or_else(|| {
        anyhow!("the configured AI permission model installer is an empty command")
    })?;
    let output = Command::new(program)
        .args(args)
        .env("LAYA_HOME", &ai_permissions.home)
        .env("LAYA_CHECKPOINTS", checkpoints.as_str())
        .env("LAYA_WHEEL_URL", &release.wheel_url)
        .env("LAYA_RELEASE", &release.tag)
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

/// The virtual environment the wheel goes into, created where there is none.
/// Its interpreter is what everything after this runs.
async fn venv(ai_permissions: &AiPermissions) -> Result<PathBuf> {
    let venv = ai_permissions.home.join("venv");
    let interpreter = venv.join("bin").join("python");
    if interpreter.is_file() {
        return Ok(venv);
    }
    let python = super::python::probe_python(
        ai_permissions.python_bin.as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .await;
    let (Some(path), true) = (python.path.as_deref(), python.ok) else {
        bail!(
            "the AI permission model needs Python 3.10 or newer; this daemon found {}",
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

/// The package itself, taken from the release's wheel, with the `serve` extra
/// that carries `laya-serve`.
async fn wheel(venv: &Path, release: &Release) -> Result<()> {
    run_to_completion(
        Command::new(venv.join("bin").join("python")).args([
            "-m",
            "pip",
            "install",
            "--upgrade",
            &format!("laya[serve] @ {}", release.wheel_url),
        ]),
        "installing the AI permission model wheel",
    )
    .await
}

/// The checkpoints, downloaded by the model's own router into a Hugging Face cache
/// under the model home, so nothing lands in the user's.
async fn weights(
    ai_permissions: &AiPermissions,
    venv: &Path,
    checkpoints: AiPermissionsCheckpoints,
) -> Result<()> {
    let names: &[&str] = match checkpoints {
        AiPermissionsCheckpoints::English => &ENGLISH,
        AiPermissionsCheckpoints::All => &ALL,
    };
    let quoted = names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    run_to_completion(
        Command::new(venv.join("bin").join("python"))
            .arg("-c")
            .arg(format!(
                "from laya import Router; Router().preload([{quoted}])"
            ))
            .env("HF_HOME", ai_permissions.home.join("hf")),
        "downloading the AI permission model checkpoints",
    )
    .await
}

/// Run one step of the install. A step that fails carries what it printed on
/// stderr, which is what a user is left to act on.
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

/// How a process ended, for a failure that printed nothing.
fn ended(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exited with status {code}"),
        None => "was killed by a signal".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document is read for the tag and the wheel, and a document that
    /// carries neither is refused where it is rather than at the pip step.
    #[tokio::test]
    async fn the_release_document_gives_the_tag_and_the_wheel() {
        let served = |body: serde_json::Value| async move {
            let app = axum::Router::new().route(
                "/release.json",
                axum::routing::get(move || {
                    let body = body.clone();
                    async move { axum::Json(body) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/release.json", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let found = release(&url, Duration::from_secs(5)).await;
            task.abort();
            found
        };

        let good = served(serde_json::json!({
            "tag_name": "v0.1.4",
            "assets": [
                {"browser_download_url": "https://example.test/model-0.1.4.tar.gz"},
                {"browser_download_url": "https://example.test/model-0.1.4-py3-none-any.whl"},
            ],
        }))
        .await
        .unwrap();
        assert_eq!(
            good,
            Release {
                tag: "v0.1.4".into(),
                wheel_url: "https://example.test/model-0.1.4-py3-none-any.whl".into(),
            }
        );

        // A release with only the tarball is one nothing can be installed
        // from, and it says so naming the tag it read.
        let wheelless = served(serde_json::json!({
            "tag_name": "v0.1.4",
            "assets": [{"browser_download_url": "https://example.test/model-0.1.4.tar.gz"}],
        }))
        .await
        .unwrap_err();
        assert!(format!("{wheelless:#}").contains("v0.1.4"), "{wheelless:#}");

        let tagless = served(serde_json::json!({"assets": []})).await.unwrap_err();
        assert!(format!("{tagless:#}").contains("tag_name"), "{tagless:#}");
    }
}
