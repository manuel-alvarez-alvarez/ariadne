//! Populate only explicitly selected repositories from Git remotes.
use super::*;
use crate::http::convert::repository_dto;
use ariadne_api::repositories::RepositoryDto;

impl Hosts {
    async fn worker_repositories(
        &self,
        request: &SetupRequest,
        cli: &str,
    ) -> Result<Vec<RepositoryDto>, String> {
        let json = self.ssh(request, &format!("{cli} repo ls"), None).await?;
        serde_json::from_str(&json)
            .map_err(|_| "cannot read worker repository registrations".into())
    }

    pub(super) async fn setup_repositories(
        &self,
        name: &str,
        request: &SetupRequest,
        home: &str,
        remote: &str,
        path: &str,
    ) -> Result<(), String> {
        if request.repositories.is_empty() {
            return Ok(());
        }
        let cli = format!(
            "PATH={} {} --host local --endpoint {} --format json",
            quote(path),
            quote(&format!("{home}/.local/bin/ariadne")),
            quote(remote)
        );
        if let Some(id) = request.repositories.keys().find(|id| {
            !self.0.setup.lock().unwrap()[name]
                .progress
                .steps
                .iter()
                .any(|s| s.name == format!("repository:{id}") && s.complete)
        }) {
            self.begin(name, &format!("repository:{id}"))?;
        }
        let registered = self.worker_repositories(request, &cli).await?;
        let mut selected = Vec::new();
        for id in request.repositories.keys() {
            selected.push(repository_dto(
                self.0
                    .store
                    .get_repository(id)
                    .await
                    .map_err(|_| "selected source repository no longer exists")?,
            ));
        }
        for pair in super::super::sync::pair(selected, registered.clone()) {
            let source = pair.local;
            let id = &source.id;
            let destination = &request.repositories[id];
            let step = format!("repository:{id}");
            if self.0.setup.lock().unwrap()[name]
                .progress
                .steps
                .iter()
                .any(|s| s.name == step && s.complete)
            {
                continue;
            }
            self.begin(name, &step)?;
            let destination = destination
                .strip_prefix("~/")
                .map(|p| format!("{home}/{p}"))
                .unwrap_or_else(|| destination.clone());
            let clone_step = format!("clone:{id}");
            let cloned = self.0.setup.lock().unwrap()[name]
                .progress
                .steps
                .iter()
                .any(|s| s.name == clone_step && s.complete);
            let registered_clone = cloned
                && registered
                    .iter()
                    .any(|r| r.path == destination && r.base_branch == source.base_branch);
            if pair.remote.is_none() && !registered_clone {
                if !cloned {
                    self.begin(name, &clone_step)?;
                    let remotes = self
                        .command(
                            Command::new("git").args(["-C", &source.path, "remote"]),
                            None,
                        )
                        .await
                        .map_err(|_| "cannot read source Git remotes")?;
                    let remote_name = source
                        .forge
                        .as_ref()
                        .map(|f| f.remote.as_str())
                        .or_else(|| remotes.lines().find(|r| *r == "origin"))
                        .or_else(|| remotes.lines().next())
                        .ok_or("source repository has no Git remote")?;
                    let url = self
                        .command(
                            Command::new("git").args([
                                "-C",
                                &source.path,
                                "remote",
                                "get-url",
                                remote_name,
                            ]),
                            None,
                        )
                        .await
                        .map_err(|_| "source repository has no accessible Git remote")?;
                    let url = url.trim();
                    if url.is_empty() || url.chars().any(char::is_control) {
                        return Err("source repository has no usable Git remote".into());
                    }
                    let parent = Path::new(&destination)
                        .parent()
                        .ok_or("destination has no parent")?
                        .to_string_lossy();
                    let marker = format!(
                        "{home}/.ariadne/repository-setup/{name}/{}",
                        hex::encode(id)
                    );
                    let script = format!(
                        r#"set -eu
umask 077
export PATH={path}
export GIT_TERMINAL_PROMPT=0
export GIT_SSH_COMMAND="${{GIT_SSH_COMMAND:-ssh -oBatchMode=yes}}"
destination={destination}
marker={marker}
mkdir -p -- {parent} "${{marker%/*}}"
if [ -e "$marker" ]; then
    [ "$(cat "$marker")" = "$destination" ] || exit 1
else
    mkdir -- "$destination"
    printf '%s' "$destination" > "$marker"
fi
if [ ! -f "$marker.complete" ]; then
    git clone --no-local --branch {branch} -- {url} "$destination"
    touch "$marker.complete"
fi
"#,
                        path = quote(path),
                        destination = quote(&destination),
                        marker = quote(&marker),
                        parent = quote(&parent),
                        branch = quote(&source.base_branch),
                        url = quote(url)
                    );
                    self.ssh(request, &script, None).await.map_err(|_| "clone failed: check the destination, Git remote, and worker Git authentication; existing directories are never overwritten")?;
                    self.checkpoint(name, &clone_step)?;
                }
                self.begin(name, &step)?;
                self.ssh(
                    request,
                    &format!(
                        "{cli} repo add {} --branch {}",
                        quote(&destination),
                        quote(&source.base_branch)
                    ),
                    None,
                )
                .await
                .map_err(
                    |_| "worker repository registration failed; retry retains the completed clone",
                )?;
            } else {
                self.checkpoint(name, &format!("clone:{id}"))?;
            }
            self.checkpoint(name, &step)?;
        }
        Ok(())
    }
}
