//! Durable, resumable SSH worker setup. Each remote command is noninteractive.
use super::*;
use ariadne_api::hosts::{
    BinarySource, HostSetupDto, SetupDto, SetupOperation, SetupRequest, SetupState, SetupStep,
    UpdateRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

mod build;
mod inspect;
mod plan;
mod populate;
mod repositories;
use populate::selection_steps;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Record {
    pub request: SetupRequest,
    #[serde(default = "with_binaries")]
    binaries: bool,
    pub progress: SetupDto,
    #[serde(default)]
    active_step: Option<String>,
    /// The worker's platform target, once the probe has read it.
    #[serde(default)]
    target: Option<String>,
    /// The successful steps that found their item present and installed nothing.
    #[serde(default)]
    present: std::collections::BTreeSet<String>,
    /// The worker's system package manager, once a source build has looked.
    #[serde(default)]
    manager: Option<String>,
}

fn with_binaries() -> bool {
    true
}

/// The steps that bring the Ariadne programs to the worker after `validate`:
/// a copy of finished programs, or a source build on the worker.
fn program_steps(build: bool) -> &'static [&'static str] {
    if build {
        &[
            "prerequisite:compiler",
            "prerequisite:curl",
            "source",
            "rust",
            "build",
        ]
    } else {
        &["transfer"]
    }
}

fn step(name: String) -> SetupStep {
    SetupStep {
        name,
        complete: false,
        error: None,
        title: String::new(),
        manual: None,
        group: None,
        skipped: false,
    }
}

/// Whether the user can skip the step once it fails: an external tool, a
/// credential copy or a sign-in check. Git and the steps of Ariadne itself
/// cannot be skipped.
fn skippable(step: &str) -> bool {
    step.strip_prefix("tool:").is_some_and(|id| id != "git")
        || step.starts_with("credential:")
        || step.starts_with("authentication:")
}

fn steps(request: &SetupRequest, update: bool, binaries: bool, build: bool) -> Vec<SetupStep> {
    ["probe"]
        .into_iter()
        .map(String::from)
        .chain((update).then_some("worker-capability".into()))
        .chain(binaries.then_some("validate".into()))
        .chain(
            program_steps(build)
                .iter()
                .filter(|_| binaries)
                .map(|name| name.to_string()),
        )
        .chain((update && binaries).then_some("replacement-capability".into()))
        .chain(update.then_some("maintenance".into()))
        .chain(binaries.then_some("install".into()))
        .chain((!update && request.tools.is_none()).then_some("prerequisite:git".into()))
        .chain(selection_steps(request))
        .chain(
            request
                .repositories
                .keys()
                .flat_map(|id| [format!("clone:{id}"), format!("repository:{id}")]),
        )
        .chain(
            ["service", "health", "register"]
                .into_iter()
                .map(String::from),
        )
        .chain(update.then_some("resume".into()))
        .map(step)
        .collect()
}

pub(super) fn load(root: &Path) -> BTreeMap<String, Record> {
    let mut records: BTreeMap<String, Record> = std::fs::read(root.join("worker-setup.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    for record in records.values_mut() {
        if matches!(
            record.progress.state,
            SetupState::Running | SetupState::Pending
        ) {
            record.progress.state = SetupState::Incomplete;
        }
    }
    records
}

fn save(root: &Path, records: &BTreeMap<String, Record>) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join("worker-setup.json");
    let temp = root.join("worker-setup.json.tmp");
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(records).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())?;
    std::fs::File::open(root)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())
}

fn validate(name: &str, request: &SetupRequest) -> Result<(), String> {
    for (id, destination) in &request.repositories {
        if id.is_empty()
            || id.chars().any(char::is_control)
            || !(destination.starts_with('/') || destination.starts_with("~/"))
            || destination.chars().any(char::is_control)
        {
            return Err("select repository IDs and absolute or ~/ worker destinations".into());
        }
    }
    if name.is_empty()
        || matches!(name, "local" | "all")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("use a worker name with letters, digits, hyphens or underscores; local and all are reserved".into());
    }
    let url = Url::parse(&request.ssh).map_err(|_| "invalid SSH address")?;
    if url.scheme() != "ssh"
        || !request.ssh.starts_with("ssh://")
        || url.host_str().is_none()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.username().starts_with('-')
    {
        return Err("use ssh://[user@]host[:port] without passwords or paths".into());
    }
    if let Some(remote) = &request.remote
        && (!remote.starts_with('/') || remote.contains(['\n', '\r', '\0']))
    {
        return Err("remote must be an absolute socket path".into());
    }
    if let BinarySource::Release { repository, tag } = &request.source
        && (repository.split('/').count() != 2
            || !repository
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/-_.".contains(&c))
            || tag.is_empty()
            || !tag
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            || tag.starts_with('-'))
    {
        return Err("select an owner/repository and an explicit release tag".into());
    }
    if request.credentials.iter().any(|id| {
        !matches!(
            id.as_str(),
            "codex" | "github" | "claude" | "opencode" | "ssh"
        )
    }) {
        return Err(
            "select a supported credential reference; credential values are not accepted".into(),
        );
    }
    for ids in [
        request
            .tools
            .as_ref()
            .map(|t| t.agents.as_slice())
            .unwrap_or_default(),
        request.credentials.as_slice(),
    ] {
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            return Err("select each tool and credential reference only once".into());
        }
    }
    for id in request
        .tools
        .iter()
        .flat_map(|t| &t.agents)
        .chain(&request.credentials)
    {
        if id.is_empty()
            || id.len() > 80
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(
                "select credential references and agent IDs, never credential values".into(),
            );
        }
    }
    Ok(())
}

/// Why a saved setup was not discarded.
pub(crate) enum DiscardError {
    Unknown,
    /// The setup is pending, running or complete; the word names which.
    Active(&'static str),
    Save(String),
}

/// Why a failed step was not skipped.
pub(crate) enum SkipError {
    /// No setup or update of the machine, or no such step in it.
    Unknown(String),
    /// The step cannot be skipped, or it has not failed.
    Refused(String),
    /// The setup is pending or running.
    Running,
    Save(String),
}

impl Hosts {
    pub(crate) fn setup_status(&self, name: &str) -> Option<SetupDto> {
        self.0
            .setup
            .lock()
            .unwrap()
            .get(name)
            .map(|r| r.view(&self.0.cfg))
    }

    /// Every saved setup and update, by saved name.
    pub(crate) fn setups(&self) -> Vec<HostSetupDto> {
        self.0
            .setup
            .lock()
            .unwrap()
            .iter()
            .map(|(key, r)| HostSetupDto {
                name: r.progress.name.clone(),
                operation: if key.starts_with("update:") {
                    SetupOperation::Update
                } else {
                    SetupOperation::Setup
                },
                ssh: r.request.ssh.clone(),
                state: r.progress.state,
                steps: r.progress.steps.clone(),
                tools: r.request.tools.clone(),
            })
            .collect()
    }

    /// Delete the saved record of a stopped setup. The worker is not contacted.
    pub(crate) fn discard_setup(&self, name: &str) -> Result<(), DiscardError> {
        let mut records = self.0.setup.lock().unwrap();
        let state = match records.get(name) {
            Some(record) if !name.starts_with("update:") => record.progress.state,
            _ => return Err(DiscardError::Unknown),
        };
        match state {
            SetupState::Incomplete | SetupState::Failed => {}
            SetupState::Pending => return Err(DiscardError::Active("pending")),
            SetupState::Running => return Err(DiscardError::Active("running")),
            SetupState::Complete => return Err(DiscardError::Active("complete")),
        }
        let record = records.remove(name).unwrap();
        if let Err(error) = save(&self.0.cfg.root, &records) {
            records.insert(name.to_string(), record);
            return Err(DiscardError::Save(error));
        }
        Ok(())
    }

    /// Mark one failed step of a stopped setup skipped, so that the next run
    /// goes on past it. An update of the machine that has not completed is
    /// the operation that the step belongs to; else it is the setup.
    pub(crate) fn skip_setup_step(&self, name: &str, step: &str) -> Result<SetupDto, SkipError> {
        let mut records = self.0.setup.lock().unwrap();
        let update = format!("update:{name}");
        let key = if records
            .get(&update)
            .is_some_and(|r| r.progress.state != SetupState::Complete)
        {
            update
        } else {
            name.to_string()
        };
        let record = records
            .get_mut(&key)
            .ok_or_else(|| SkipError::Unknown(format!("unknown worker setup {name}")))?;
        if matches!(
            record.progress.state,
            SetupState::Pending | SetupState::Running
        ) {
            return Err(SkipError::Running);
        }
        let found = record
            .progress
            .steps
            .iter_mut()
            .find(|s| s.name == step)
            .ok_or_else(|| SkipError::Unknown(format!("worker setup {name} has no step {step}")))?;
        if !skippable(step) {
            return Err(SkipError::Refused(format!(
                "step {step} cannot be skipped; only a failed tool, credential copy or sign-in check can be skipped"
            )));
        }
        if found.complete || found.error.is_none() {
            return Err(SkipError::Refused(format!("step {step} has not failed")));
        }
        let was = std::mem::replace(&mut found.skipped, true);
        let progress = record.view(&self.0.cfg);
        if let Err(error) = save(&self.0.cfg.root, &records) {
            let record = records.get_mut(&key).unwrap();
            if let Some(found) = record.progress.steps.iter_mut().find(|s| s.name == step) {
                found.skipped = was;
            }
            return Err(SkipError::Save(error));
        }
        Ok(progress)
    }

    pub(crate) fn resume_setup(&self, name: String) -> Result<SetupDto, String> {
        let request = self
            .0
            .setup
            .lock()
            .unwrap()
            .get(&name)
            .ok_or("Unknown worker setup. Start setup with a reviewed request first.")?
            .request
            .clone();
        self.setup(name, request)
    }

    pub(crate) fn setup(&self, name: String, request: SetupRequest) -> Result<SetupDto, String> {
        self.start_operation(name, request, false, true)
    }

    pub(crate) fn update_status(&self, name: &str) -> Option<SetupDto> {
        self.setup_status(&format!("update:{name}"))
    }

    pub(crate) fn update(&self, name: String, request: UpdateRequest) -> Result<SetupDto, String> {
        let connection = self.connection(&name).ok_or("unknown worker")?;
        if !connection.endpoint.starts_with("ssh://") {
            return Err("select an SSH worker".into());
        }
        if request.source.is_none() && request.tools.is_none() {
            return Err("select binaries, tools, or both".into());
        }
        let binaries = request.source.is_some();
        let request = SetupRequest {
            ssh: connection.endpoint,
            remote: connection.remote,
            // Tool-only records share the setup runner but never read this placeholder source.
            source: request.source.unwrap_or(BinarySource::Local {
                cli: String::new(),
                daemon: String::new(),
            }),
            tools: request.tools,
            credentials: vec![],
            repositories: BTreeMap::new(),
        };
        self.start_operation(name, request, true, binaries)
    }

    fn start_operation(
        &self,
        worker: String,
        request: SetupRequest,
        update: bool,
        binaries: bool,
    ) -> Result<SetupDto, String> {
        validate(&worker, &request)?;
        let build = self.source_build(&request, binaries);
        let name = if update {
            format!("update:{worker}")
        } else {
            worker.clone()
        };
        let mut records = self.0.setup.lock().unwrap();
        let other = if update {
            worker.clone()
        } else {
            format!("update:{worker}")
        };
        if records.get(&other).is_some_and(|r| {
            matches!(
                r.progress.state,
                SetupState::Pending | SetupState::Running | SetupState::Incomplete
            )
        }) {
            return Err("finish the existing worker operation first".into());
        }
        if update
            && records
                .get(&name)
                .is_some_and(|r| r.progress.state == SetupState::Complete)
        {
            records.remove(&name);
        }
        let path = self.0.cfg.root.join("worker-setup.json");
        if path.exists() {
            let bytes = std::fs::read(path).map_err(|_| "cannot read stored worker setup")?;
            serde_json::from_slice::<BTreeMap<String, Record>>(&bytes).map_err(|_| "stored worker setup is invalid; preserve the file and repair it before retrying")?;
        }
        if let Some(record) = records.get_mut(&name) {
            if update {
                if matches!(
                    record.progress.state,
                    SetupState::Pending | SetupState::Running
                ) {
                    if record.request != request || record.binaries != binaries {
                        return Err("wait for the active update before changing selections".into());
                    }
                    return Ok(record.view(&self.0.cfg));
                }
                let source_changed =
                    record.request.source != request.source || record.binaries != binaries;
                let connection_changed =
                    record.request.ssh != request.ssh || record.request.remote != request.remote;
                let old_steps = &record.progress.steps;
                let new_steps = steps(&request, true, binaries, build)
                    .into_iter()
                    .map(|mut step| {
                        let affected = connection_changed
                            || (source_changed
                                && matches!(
                                    step.name.as_str(),
                                    "validate"
                                        | "transfer"
                                        | "prerequisite:compiler"
                                        | "prerequisite:curl"
                                        | "source"
                                        | "rust"
                                        | "build"
                                        | "replacement-capability"
                                        | "install"
                                        | "service"
                                        | "health"
                                        | "register"
                                        | "resume"
                                ));
                        if !affected
                            && let Some(old) = old_steps.iter().find(|old| old.name == step.name)
                        {
                            step.complete = old.complete;
                            if old.skipped {
                                step.skipped = true;
                                step.error = old.error.clone();
                            }
                        }
                        step
                    })
                    .collect();
                record.request = request.clone();
                record.binaries = binaries;
                record.progress.steps = new_steps;
                record.active_step = None;
            } else {
                if record.request != request {
                    let mut previous = record.request.clone();
                    previous.repositories = request.repositories.clone();
                    if record.progress.state != SetupState::Complete || previous != request {
                        return Err("retry with the original setup request; change repository selections only after setup completes".into());
                    }
                    record.request = request.clone();
                    record.progress.state = SetupState::Incomplete;
                    for id in request.repositories.keys() {
                        for name in [format!("clone:{id}"), format!("repository:{id}")] {
                            if !record.progress.steps.iter().any(|s| s.name == name) {
                                record.progress.steps.push(step(name));
                            }
                        }
                    }
                }
            }
            if matches!(
                record.progress.state,
                SetupState::Running | SetupState::Pending | SetupState::Complete
            ) {
                return Ok(record.view(&self.0.cfg));
            }
        } else {
            if self.list().iter().any(|h| h.name == name)
                && !self.0.cfg.hosts.iter().any(|(n, host)| {
                    n == &name && host.endpoint == request.ssh && host.remote == request.remote
                })
            {
                return Err(
                    "existing worker updates must use its configured SSH address and socket".into(),
                );
            }
            records.insert(
                name.clone(),
                Record {
                    request: request.clone(),
                    binaries,
                    active_step: None,
                    target: None,
                    present: Default::default(),
                    manager: None,
                    progress: SetupDto {
                        name: worker.clone(),
                        state: SetupState::Pending,
                        steps: steps(&request, update, binaries, build),
                    },
                },
            );
        }
        let record = records.get_mut(&name).unwrap();
        // Older incomplete operations did not have a separate Git prerequisite.
        if !update
            && request.tools.is_none()
            && !record
                .progress
                .steps
                .iter()
                .any(|step| step.name == "prerequisite:git")
        {
            record
                .progress
                .steps
                .insert(1, step("prerequisite:git".into()));
        }
        // A saved operation can name the other way to bring the programs: one
        // saved before source builds, or one this daemon's build changed.
        let wanted = program_steps(build);
        if record.binaries
            && !wanted
                .iter()
                .all(|name| record.progress.steps.iter().any(|s| s.name == *name))
        {
            let other = program_steps(!build);
            record.progress.steps.retain(|s| {
                !other.contains(&s.name.as_str()) && !wanted.contains(&s.name.as_str())
            });
            let at = record
                .progress
                .steps
                .iter()
                .position(|s| s.name == "validate")
                .map_or(1, |at| at + 1);
            for (offset, name) in wanted.iter().enumerate() {
                record
                    .progress
                    .steps
                    .insert(at + offset, step(name.to_string()));
            }
            if let Some(install) = record
                .progress
                .steps
                .iter_mut()
                .find(|s| s.name == "install")
            {
                install.complete = false;
            }
        }
        record.progress.state = SetupState::Pending;
        // A skipped step keeps its error, so its manual action stays in view.
        for step in record.progress.steps.iter_mut().filter(|s| !s.skipped) {
            step.error = None;
        }
        let progress = record.view(&self.0.cfg);
        save(&self.0.cfg.root, &records)?;
        drop(records);
        let this = self.clone();
        self.0.tasks.lock().unwrap().push(tokio::spawn(async move {
            if let Err(error) = this.run_setup(&name, &request).await {
                if update {
                    this.recover_update(&name, &request).await;
                }
                let mut records = this.0.setup.lock().unwrap();
                let record = records.get_mut(&name).unwrap();
                record.progress.state = if record.progress.steps.iter().any(|s| s.complete) {
                    SetupState::Incomplete
                } else {
                    SetupState::Failed
                };
                if let Some(step) = record
                    .progress
                    .steps
                    .iter_mut()
                    .find(|s| Some(&s.name) == record.active_step.as_ref())
                {
                    step.error = Some(error);
                }
                if let Err(error) = save(&this.0.cfg.root, &records) {
                    warn!(%error, "cannot save worker failure");
                }
            }
        }));
        Ok(progress)
    }

    async fn recover_update(&self, name: &str, request: &SetupRequest) {
        let held = {
            let records = self.0.setup.lock().unwrap();
            let record = &records[name];
            record.progress.steps.iter().any(|s| {
                s.name == "maintenance"
                    && (s.complete || record.active_step.as_deref() == Some("maintenance"))
            })
        };
        if !held {
            return;
        }
        // Recovery never changes the primary failing step or its error.
        let result = async {
            let cli = self.worker_cli(request).await?;
            let remote = request.remote.clone().unwrap_or_else(|| {
                cli.trim_end_matches("/.local/bin/ariadne").to_owned() + "/.ariadne/ariadne.sock"
            });
            self.ssh(
                request,
                &format!(
                    "{} --endpoint {} daemon status >/dev/null 2>&1",
                    quote(&cli),
                    quote(&remote)
                ),
                None,
            )
            .await?;
            self.maintenance(request, &cli, &remote, Some(false))
                .await?;
            Ok::<_, String>(())
        }
        .await;
        let mut records = self.0.setup.lock().unwrap();
        if let Some(step) = records
            .get_mut(name)
            .unwrap()
            .progress
            .steps
            .iter_mut()
            .find(|s| s.name == "resume")
        {
            step.complete = result.is_ok();
            step.error = result.err().map(|_| "worker health or maintenance release failed; scheduling remains held; repair the worker and retry".into());
        }
    }

    async fn worker_cli(&self, request: &SetupRequest) -> Result<String, String> {
        let home = self.ssh(request, "printf '%s' \"$HOME\"", None).await?;
        if !home.starts_with('/') || home.chars().any(char::is_control) {
            return Err("invalid worker home".into());
        }
        Ok(format!("{home}/.local/bin/ariadne"))
    }

    fn begin(&self, name: &str, step: &str) -> Result<(), String> {
        let mut records = self.0.setup.lock().unwrap();
        let record = records.get_mut(name).unwrap();
        record.progress.state = SetupState::Running;
        record.active_step = Some(step.into());
        if let Some(result) = record.progress.steps.iter_mut().find(|s| s.name == step) {
            result.complete = false;
        }
        save(&self.0.cfg.root, &records)
    }

    fn checkpoint(&self, name: &str, step: &str) -> Result<(), String> {
        let mut records = self.0.setup.lock().unwrap();
        let record = records.get_mut(name).unwrap();
        record.progress.state = SetupState::Running;
        record
            .progress
            .steps
            .iter_mut()
            .find(|s| s.name == step)
            .unwrap()
            .complete = true;
        save(&self.0.cfg.root, &records)
    }

    async fn command(
        &self,
        command: &mut Command,
        input: Option<Vec<u8>>,
    ) -> Result<String, String> {
        self.command_within(command, input, self.0.timeouts.worker_setup_command)
            .await
    }

    async fn command_within(
        &self,
        command: &mut Command,
        input: Option<Vec<u8>>,
        limit: std::time::Duration,
    ) -> Result<String, String> {
        command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start setup command: {e}"))?;
        let mut stdin = child.stdin.take().unwrap();
        let run = async move {
            let write = async move {
                if let Some(input) = input {
                    stdin.write_all(&input).await.map_err(|e| e.to_string())?;
                }
                drop(stdin);
                Ok::<_, String>(())
            };
            let (written, output) = tokio::join!(write, child.wait_with_output());
            written?;
            let output = output.map_err(|e| e.to_string())?;
            // Remote stderr can contain secrets from local SSH configuration. Never return it.
            if !output.status.success() {
                return Err(format!(
                    "setup command failed ({}); check SSH access, prerequisites, and the user service",
                    output.status
                ));
            }
            String::from_utf8(output.stdout).map_err(|_| "setup returned invalid text".into())
        };
        timeout(limit, run)
            .await
            .map_err(|_| "setup command timed out".to_owned())?
    }

    async fn ssh(
        &self,
        request: &SetupRequest,
        script: &str,
        input: Option<Vec<u8>>,
    ) -> Result<String, String> {
        self.ssh_within(request, script, input, self.0.timeouts.worker_setup_command)
            .await
    }

    async fn ssh_within(
        &self,
        request: &SetupRequest,
        script: &str,
        input: Option<Vec<u8>>,
        limit: std::time::Duration,
    ) -> Result<String, String> {
        let url = Url::parse(&request.ssh).map_err(|e| e.to_string())?;
        let mut command = Command::new(self.0.cfg.ssh_bin.as_deref().unwrap_or("ssh"));
        command.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
        ]);
        if let Some(port) = url.port() {
            command.args(["-p", &port.to_string()]);
        }
        command.arg("--").arg(if url.username().is_empty() {
            url.host_str().unwrap().to_owned()
        } else {
            format!("{}@{}", url.username(), url.host_str().unwrap())
        });
        command.arg(format!("sh -c {}", quote(script)));
        self.command_within(&mut command, input, limit).await
    }

    async fn run_setup(&self, name: &str, request: &SetupRequest) -> Result<(), String> {
        let worker = self.0.setup.lock().unwrap()[name].progress.name.clone();
        let update = name.starts_with("update:");
        self.begin(name, "probe")?;
        // The first three lines identify the platform and home. The remaining bytes
        // carry the selected PATH verbatim, without an added terminator.
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| "cannot create remote PATH markers")?;
        let token = hex::encode(random);
        let start = format!("__ARIADNE_PATH_{token}_START__");
        let end = format!("__ARIADNE_PATH_{token}_END__");
        let path_command = quote(&format!("printf '%s%s%s' '{start}' \"$PATH\" '{end}'"));
        let script = format!(
            r#"set -eu
uname -s
uname -m
printf '%s\n' "$HOME"
path_start='{start}'
path_end='{end}'
if remote_path=$("${{SHELL:-/bin/sh}}" -lc {path_command} 2>/dev/null); then
    case "$remote_path" in
        *"$path_start"*"$path_end"*)
            remote_path=${{remote_path#*"$path_start"}}
            remote_path=${{remote_path%%"$path_end"*}}
            if [ -n "$remote_path" ]; then
                printf '%s' "$remote_path"
            else
                printf '%s' "$PATH"
            fi
            ;;
        *) printf '%s' "$PATH" ;;
    esac
else
    printf '%s' "$PATH"
fi
"#
        );
        let probe = self.ssh(request, &script, None).await.map_err(|_| {
            "SSH probe failed; check SSH access and the worker shell configuration".to_owned()
        })?;
        let lines: Vec<_> = probe.splitn(4, '\n').collect();
        if lines.len() != 4 || !lines[2].starts_with('/') || lines[2].chars().any(char::is_control)
        {
            return Err("SSH probe failed; check the worker platform and home directory".into());
        }
        let captured_path = lines[3];
        if captured_path.chars().any(char::is_control) {
            return Err("remote PATH contains a control character; correct the remote shell configuration before retrying".into());
        }
        let target = match (lines[0], lines[1]) {
            ("Darwin", "arm64" | "aarch64") => "aarch64-apple-darwin",
            ("Darwin", "x86_64") => "x86_64-apple-darwin",
            ("Linux", "aarch64" | "arm64") => "aarch64-unknown-linux-gnu",
            ("Linux", "x86_64") => "x86_64-unknown-linux-gnu",
            _ => {
                return Err(
                    "unsupported worker platform; use macOS or Linux on ARM64 or x86_64".into(),
                );
            }
        };
        self.0.setup.lock().unwrap().get_mut(name).unwrap().target = Some(target.into());
        self.checkpoint(name, "probe")?;
        let home = lines[2];
        let service_path = if captured_path.is_empty() {
            format!("{home}/.local/bin")
        } else {
            format!("{home}/.local/bin:{captured_path}")
        };
        let remote = request
            .remote
            .clone()
            .unwrap_or_else(|| format!("{home}/.ariadne/ariadne.sock"));
        let worker_cli = format!("{home}/.local/bin/ariadne");
        if update {
            self.begin(name, "worker-capability")?;
            self.maintenance(request, &worker_cli, &remote, None)
                .await?;
            self.checkpoint(name, "worker-capability")?;
        }
        let binaries = self.0.setup.lock().unwrap()[name].binaries;
        let installed = self.0.setup.lock().unwrap()[name]
            .progress
            .steps
            .iter()
            .any(|s| s.name == "install" && s.complete);
        let stage = format!("{home}/.ariadne/worker-setup-{name}");
        if self.source_build(request, binaries) {
            // Refuse before any transfer when this machine cannot pack the commit.
            self.begin(name, "validate")?;
            self.0.cfg.build.check_source().await?;
            self.checkpoint(name, "validate")?;
            if !installed {
                if !self
                    .build_prerequisites(name, request, lines[0], &service_path)
                    .await?
                {
                    self.populate(name, request, &service_path).await?;
                    let mut records = self.0.setup.lock().unwrap();
                    records.get_mut(name).unwrap().progress.state = SetupState::Incomplete;
                    return save(&self.0.cfg.root, &records);
                }
                self.build_programs(name, request, &service_path, &stage)
                    .await?;
            }
        } else if binaries {
            self.begin(name, "validate")?;
            let selected = async {
                let source = self.binaries(name, request, target).await?;
                let cli =
                    std::fs::read(&source.0).map_err(|_| "Cannot read the local CLI binary.")?;
                let daemon =
                    std::fs::read(&source.1).map_err(|_| "Cannot read the local daemon binary.")?;
                if !compatible(&cli, target) || !compatible(&daemon, target) {
                    return Err(format!(
                        "both binaries must be compatible with {target}; nothing was transferred"
                    ));
                }
                Ok::<_, String>((cli, daemon))
            }
            .await;
            let (cli, daemon) = match selected {
                Ok(selected) => selected,
                Err(error) if !update => {
                    self.population_result(name, "validate", Err(error))?;
                    self.populate(name, request, &service_path).await?;
                    let mut records = self.0.setup.lock().unwrap();
                    records.get_mut(name).unwrap().progress.state = SetupState::Incomplete;
                    return save(&self.0.cfg.root, &records);
                }
                Err(error) => return Err(error),
            };
            self.checkpoint(name, "validate")?;
            if !installed {
                self.begin(name, "transfer")?;
                for (binary, data) in [("ariadne", cli), ("ariadned", daemon)] {
                    self.ssh(
                        request,
                        &format!(
                            "set -eu; umask 077; mkdir -p {}; cat > {}",
                            quote(&stage),
                            quote(&format!("{stage}/{binary}"))
                        ),
                        Some(data),
                    )
                    .await?;
                }
                self.checkpoint(name, "transfer")?;
            }
        }
        if update && binaries && !installed {
            self.begin(name, "replacement-capability")?;
            let protocol = self.ssh(request, &format!(
                "set -eu; chmod 755 {stage}/ariadne {stage}/ariadned; {stage}/ariadne daemon maintenance --help >/dev/null; {stage}/ariadned --maintenance-protocol",
                stage=quote(&stage)), None).await.map_err(|_| "replacement lacks maintenance support; select a supported release or compatible local binaries".to_owned())?;
            if protocol.trim() != "1" {
                return Err("replacement maintenance protocol is unsupported; worker binaries were preserved".into());
            }
            self.checkpoint(name, "replacement-capability")?;
        }
        if update {
            self.begin(name, "maintenance")?;
            loop {
                if self
                    .maintenance(request, &worker_cli, &remote, Some(true))
                    .await?
                {
                    break;
                }
                tokio::time::sleep(self.0.timeouts.worker_maintenance).await;
            }
            self.checkpoint(name, "maintenance")?;
        }
        if binaries && !installed {
            self.begin(name, "install")?;
            self.ssh(request, &format!("set -eu; mkdir -p {bin}; chmod 755 {stage}/ariadne {stage}/ariadned; mv -f {stage}/ariadne {bin}/ariadne; mv -f {stage}/ariadned {bin}/ariadned; rmdir {stage}", bin=quote(&format!("{home}/.local/bin")), stage=quote(&stage)), None).await?;
            self.checkpoint(name, "install")?;
        }
        self.populate(name, request, &service_path).await?;
        self.begin(name, "service")?;
        self.ssh(request, &service_script(home, lines[0], &service_path), None).await.map_err(|_| -> String { if lines[0] == "Linux" { "user service unavailable; ask the administrator to install systemd user services and enable lingering".into() } else { "launchd user service unavailable; check the user's launchd session".into() } })?;
        self.checkpoint(name, "service")?;
        self.begin(name, "health")?;
        self.ssh(
            request,
            &format!(
                "set -eu; for attempt in 1 2 3 4 5 6 7 8 9 10; do if {} --endpoint {} daemon status >/dev/null 2>&1; then exit 0; fi; sleep 1; done; exit 1",
                quote(&format!("{home}/.local/bin/ariadne")),
                quote(&remote)
            ),
            None,
        )
        .await?;
        self.checkpoint(name, "health")?;
        self.setup_repositories(name, request, home, &remote, &service_path)
            .await?;
        if !update
            && self.0.setup.lock().unwrap()[name]
                .progress
                .steps
                .iter()
                .any(|s| s.error.is_some() && !s.skipped)
        {
            let mut records = self.0.setup.lock().unwrap();
            records.get_mut(name).unwrap().progress.state = SetupState::Incomplete;
            return save(&self.0.cfg.root, &records);
        }
        self.begin(name, "register")?;
        if !update {
            let _mutation = self.mutation().await;
            let color = self
                .list()
                .iter()
                .find(|h| h.name == worker)
                .and_then(|h| h.color);
            self.save_connection(&worker, Some(&request.ssh), Some(&remote), color)
                .await?;
        }
        self.checkpoint(name, "register")?;
        if !request.repositories.is_empty() {
            super::sync::after_registration(self, &worker).await?;
        }
        if update {
            self.begin(name, "resume")?;
            self.maintenance(request, &worker_cli, &remote, Some(false))
                .await?;
            self.checkpoint(name, "resume")?;
        }
        let mut records = self.0.setup.lock().unwrap();
        records.get_mut(name).unwrap().progress.state = SetupState::Complete;
        save(&self.0.cfg.root, &records)
    }

    async fn maintenance(
        &self,
        request: &SetupRequest,
        cli: &str,
        remote: &str,
        active: Option<bool>,
    ) -> Result<bool, String> {
        let action = active
            .map(|active| format!(" --active {active}"))
            .unwrap_or_default();
        let output = self.ssh(request, &format!("{} --endpoint {} --format json daemon maintenance{action}",
            quote(cli), quote(remote)), None).await
            .map_err(|_| "worker maintenance support is unavailable; install a maintenance-capable Ariadne version before updating".to_owned())?;
        serde_json::from_str::<serde_json::Value>(&output)
            .ok()
            .and_then(|v| v["ready"].as_bool())
            .ok_or("invalid worker maintenance response".into())
    }

    async fn binaries(
        &self,
        name: &str,
        request: &SetupRequest,
        target: &str,
    ) -> Result<(PathBuf, PathBuf), String> {
        let build = &self.0.cfg.build;
        let source = match (&request.source, &build.repository, build.tag()) {
            (BinarySource::Daemon, Some(repository), Some(tag)) => BinarySource::Release {
                repository: repository.clone(),
                tag,
            },
            (source, ..) => source.clone(),
        };
        match &source {
            BinarySource::Daemon => unreachable!("a source build builds on the worker"),
            BinarySource::Local { cli, daemon } => Ok((cli.into(), daemon.into())),
            BinarySource::Release { repository, tag } => {
                let dir = self.0.cfg.root.join("worker-downloads").join(name);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let gh = self.0.cfg.gh_bin.as_deref().unwrap_or("gh");
                let version = self
                    .command(Command::new(gh).arg("--version"), None)
                    .await?;
                let number = version.split_whitespace().nth(2).unwrap_or("");
                let parts: Vec<u32> = number.split('.').filter_map(|v| v.parse().ok()).collect();
                if parts.as_slice() < [2, 102, 0].as_slice() {
                    return Err("install GitHub CLI 2.102.0 or later to verify releases".into());
                }
                {
                    let asset = format!("ariadne-{target}.tar.gz");
                    self.command(
                        Command::new(gh)
                            .args([
                                "release",
                                "download",
                                tag,
                                "--repo",
                                repository,
                                "--pattern",
                                &asset,
                                "--clobber",
                                "--dir",
                            ])
                            .arg(&dir),
                        None,
                    )
                    .await?;
                    self.command(
                        Command::new(gh)
                            .args(["attestation", "verify"])
                            .arg(dir.join(&asset))
                            .args([
                                "--repo",
                                repository,
                                "--signer-workflow",
                                &format!("{repository}/.github/workflows/release.yml"),
                                "--source-ref",
                                &format!("refs/tags/{tag}"),
                            ]),
                        None,
                    )
                    .await.map_err(|_| "release attestation verification failed for the selected workflow and tag; nothing was transferred".to_owned())?;
                    self.command(
                        Command::new("tar")
                            .arg("-xzf")
                            .arg(dir.join(&asset))
                            .arg("-C")
                            .arg(&dir)
                            .args(["ariadne", "ariadned"]),
                        None,
                    )
                    .await?;
                }
                Ok((dir.join("ariadne"), dir.join("ariadned")))
            }
        }
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn compatible(bytes: &[u8], target: &str) -> bool {
    let arm = target.starts_with("aarch64");
    if target.ends_with("linux-gnu") {
        bytes.len() >= 64
            && &bytes[..4] == b"\x7fELF"
            && bytes[4] == 2
            && bytes[5] == 1
            && matches!(u16::from_le_bytes([bytes[16], bytes[17]]), 2 | 3)
            && u16::from_le_bytes([bytes[18], bytes[19]]) == if arm { 183 } else { 62 }
    } else {
        if macho_matches(bytes, arm) {
            return true;
        }
        let wide = bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbf]);
        if !wide && !bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]) {
            return false;
        }
        let Some(count) = bytes.get(4..8) else {
            return false;
        };
        let count = u32::from_be_bytes(count.try_into().unwrap());
        if count > 64 {
            return false;
        }
        let width = if wide { 32 } else { 20 };
        (0..count as usize).any(|index| {
            let Some(entry) = bytes.get(8 + index * width..8 + (index + 1) * width) else {
                return false;
            };
            let (offset, size) = if wide {
                (
                    u64::from_be_bytes(entry[8..16].try_into().unwrap()),
                    u64::from_be_bytes(entry[16..24].try_into().unwrap()),
                )
            } else {
                (
                    u32::from_be_bytes(entry[8..12].try_into().unwrap()) as u64,
                    u32::from_be_bytes(entry[12..16].try_into().unwrap()) as u64,
                )
            };
            let Some(end) = offset.checked_add(size) else {
                return false;
            };
            let (Ok(offset), Ok(end)) = (usize::try_from(offset), usize::try_from(end)) else {
                return false;
            };
            bytes
                .get(offset..end)
                .is_some_and(|slice| macho_matches(slice, arm))
        })
    }
}

fn macho_matches(bytes: &[u8], arm: bool) -> bool {
    bytes.len() >= 32
        && bytes[..4] == [0xcf, 0xfa, 0xed, 0xfe]
        && bytes[12..16] == [2, 0, 0, 0]
        && u32::from_le_bytes(bytes[4..8].try_into().unwrap())
            == if arm { 0x0100000c } else { 0x01000007 }
}

// Environment= performs specifier expansion, but does not expand dollar signs.
fn systemd_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "%%")
        .replace('"', "\\\"")
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn service_script(home: &str, os: &str, path: &str) -> String {
    if os == "Linux" {
        format!(
            r#"set -eu
command -v systemctl >/dev/null
command -v loginctl >/dev/null
[ "$(loginctl show-user "$(id -u)" -p Linger --value)" = yes ]
export XDG_RUNTIME_DIR="${{XDG_RUNTIME_DIR:-/run/user/$(id -u)}}"
mkdir -p "$HOME/.config/systemd/user"
unit="$HOME/.config/systemd/user/ariadned.service"
if [ ! -e "$unit" ]; then
cat > "$unit" <<'ARIADNE_UNIT'
[Unit]
Description=Ariadne worker
[Service]
ExecStart="{home}/.local/bin/ariadned"
Environment="PATH={path}"
LimitNOFILE=4096
KillMode=process
Restart=on-failure
[Install]
WantedBy=default.target
ARIADNE_UNIT
fi
systemctl --user daemon-reload
systemctl --user enable ariadned.service
systemctl --user restart ariadned.service
"#,
            home = systemd_value(home),
            path = systemd_value(path)
        )
    } else {
        let xml = xml_text(home);
        let path = xml_text(path);
        format!(
            r#"set -eu
mkdir -p "$HOME/Library/LaunchAgents"
plist="$HOME/Library/LaunchAgents/dev.ariadne.daemon.plist"
if [ ! -e "$plist" ]; then
cat > "$plist" <<'ARIADNE_PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>Label</key><string>dev.ariadne.daemon</string><key>ProgramArguments</key><array><string>{xml}/.local/bin/ariadned</string></array><key>EnvironmentVariables</key><dict><key>PATH</key><string>{path}</string></dict><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ProcessType</key><string>Interactive</string><key>SoftResourceLimits</key><dict><key>NumberOfFiles</key><integer>4096</integer></dict></dict></plist>
ARIADNE_PLIST
fi
domain="user/$(id -u)"
if launchctl print "gui/$(id -u)/dev.ariadne.daemon" >/dev/null 2>&1; then domain="gui/$(id -u)"; fi
if ! launchctl print "$domain/dev.ariadne.daemon" >/dev/null 2>&1; then launchctl bootstrap "$domain" "$plist"; fi
launchctl kickstart -k "$domain/dev.ariadne.daemon"
"#
        )
    }
}
