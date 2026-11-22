//! Populate only explicitly selected repositories from Git remotes.
use super::*;
use crate::http::convert::repository_dto;
use ariadne_api::repositories::RepositoryDto;

/// The short name of a repository, from its forge or from its folder, as the
/// desktop app names it.
pub(super) fn repository_name(repository: &RepositoryDto) -> String {
    repository
        .forge
        .as_ref()
        .map(|forge| forge.name.clone())
        .or_else(|| {
            Path::new(&repository.path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| repository.id.clone())
}

/// The SSH server of a Git remote URL, as `ssh` names it.
#[derive(Debug, PartialEq)]
struct SshHost {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

impl SshHost {
    /// The name a `known_hosts` lookup uses: `host`, or `[host]:port` off port 22.
    fn lookup(&self) -> String {
        match self.port {
            Some(port) if port != 22 => format!("[{}]:{port}", self.host),
            _ => self.host.clone(),
        }
    }

    /// The `ssh` destination that checks the host key by hand.
    fn destination(&self) -> String {
        let at = self
            .user
            .as_ref()
            .map(|u| format!("{u}@"))
            .unwrap_or_default();
        match self.port {
            Some(port) if port != 22 => format!("{at}{} -p {port}", self.host),
            _ => format!("{at}{}", self.host),
        }
    }
}

/// The SSH server of `url`, for an `ssh://` URL or an scp-like `[user@]host:path`.
/// None for any other remote, or a host name `ssh` would not take as written.
fn ssh_host(url: &str) -> Option<SshHost> {
    let (user, host, port) = if let Some(rest) = ["ssh://", "git+ssh://", "ssh+git://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
    {
        let parsed = Url::parse(&format!("ssh://{rest}")).ok()?;
        let user = (!parsed.username().is_empty()).then(|| parsed.username().to_owned());
        (user, parsed.host_str()?.to_owned(), parsed.port())
    } else if url.contains("://") {
        return None;
    } else {
        let (authority, _) = url.split_once(':')?;
        if authority.contains('/') {
            return None;
        }
        match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user.to_owned()), host.to_owned(), None),
            None => (None, authority.to_owned(), None),
        }
    };
    let plain = |s: &str| {
        !s.is_empty()
            && !s.starts_with('-')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    };
    (plain(&host) && user.as_deref().is_none_or(plain)).then_some(SshHost { user, host, port })
}

/// The host a remote URL names, for an error message; never its credentials or path.
fn remote_host(url: &str, ssh: Option<&SshHost>) -> String {
    ssh.map(|h| h.host.clone())
        .or_else(|| Url::parse(url).ok()?.host_str().map(str::to_owned))
        .unwrap_or_else(|| "the Git remote".to_owned())
}

/// What a failed clone on the worker means, from the one word its script printed.
fn clone_failure(
    outcome: &str,
    destination: &str,
    branch: &str,
    ssh: Option<&SshHost>,
    url: &str,
) -> String {
    let host = remote_host(url, ssh);
    match outcome {
        "destination" => format!(
            "clone failed: the destination {destination} already exists on the worker, and setup never overwrites a directory; choose another destination or remove it, then retry"
        ),
        "git" => {
            "clone failed: Git is not on the worker's PATH; install Git on the worker, then retry"
                .into()
        }
        "host-key" => match ssh {
            Some(ssh) => format!(
                "clone failed: the worker does not trust the SSH host key of {host}, check the worker's SSH known-hosts configuration; on the worker, run `ssh -T {}` once and check the fingerprint, then retry",
                ssh.destination()
            ),
            None => format!("clone failed: the worker does not trust the SSH host key of {host}"),
        },
        "authentication" => format!(
            "clone failed: worker Git authentication to {host} was refused; add the worker's SSH key to your account on {host}, or sign in to {host} on the worker, then retry"
        ),
        "branch" => format!("clone failed: the Git remote on {host} has no branch {branch}"),
        _ => format!(
            "clone failed: the worker's Git could not clone from {host}; check that the worker can reach it, then retry"
        ),
    }
}

impl Hosts {
    /// The `known_hosts` lines this machine's `ssh` trusts for `host`, so the
    /// worker trusts the same key rather than a key it would see first.
    async fn known_host_entries(&self, host: &SshHost) -> String {
        let mut config = Command::new(self.0.cfg.ssh_bin.as_deref().unwrap_or("ssh"));
        config.arg("-G");
        if let Some(port) = host.port {
            config.args(["-p", &port.to_string()]);
        }
        config.args(["--", &host.host]);
        let Ok(config) = self.command(&mut config, None).await else {
            return String::new();
        };
        let lookup = host.lookup();
        let mut entries: Vec<String> = Vec::new();
        let files = config.lines().filter_map(|line| {
            line.strip_prefix("userknownhostsfile ")
                .or_else(|| line.strip_prefix("globalknownhostsfile "))
        });
        for file in files.flat_map(str::split_whitespace) {
            if !Path::new(file).is_file() {
                continue;
            }
            let Ok(found) = self
                .command(
                    Command::new("ssh-keygen").args(["-F", &lookup, "-f", file]),
                    None,
                )
                .await
            else {
                continue;
            };
            for line in found.lines().map(str::trim) {
                if !line.is_empty()
                    && !line.starts_with('#')
                    && !line.chars().any(char::is_control)
                    && !entries.iter().any(|e| e == line)
                {
                    entries.push(line.to_owned());
                }
            }
        }
        entries.join("\n")
    }

    /// Name each selected repository in the step titles, while it is still registered here.
    pub(super) async fn name_repositories(&self, name: &str, request: &SetupRequest) {
        let mut names = BTreeMap::new();
        for id in request.repositories.keys() {
            if let Ok(repository) = self.0.store.get_repository(id).await {
                names.insert(id.clone(), repository_name(&repository_dto(repository)));
            }
        }
        if let Some(record) = self.0.setup.lock().unwrap().get_mut(name) {
            record.repository_names.extend(names);
        }
    }

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
            let result: Result<(), String> = async {
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
                        let host = ssh_host(url);
                        let known = match &host {
                            Some(host) => self.known_host_entries(host).await,
                            None => String::new(),
                        };
                        let lookup = host.as_ref().map(SshHost::lookup).unwrap_or_default();
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
        [ "$(cat "$marker")" = "$destination" ] || {{ echo destination; exit 0; }}
    else
        mkdir -- "$destination" 2>/dev/null || {{ echo destination; exit 0; }}
        printf '%s' "$destination" > "$marker"
    fi
    if [ ! -f "$marker.complete" ]; then
        command -v git >/dev/null 2>&1 || {{ echo git; exit 0; }}
        known={known}
        if [ -n "$known" ]; then
            [ -d "$HOME/.ssh" ] || mkdir -m 700 -- "$HOME/.ssh"
            if ! ssh-keygen -F {lookup} -f "$HOME/.ssh/known_hosts" >/dev/null 2>&1; then
                f="$HOME/.ssh/known_hosts"
                [ ! -s "$f" ] || [ -z "$(tail -c 1 "$f")" ] || printf '\n' >> "$f"
                printf '%s\n' "$known" >> "$f"
            fi
        fi
        log="$marker.log"
        if ! git clone --no-local --branch {branch} -- {url} "$destination" 2>"$log"; then
            if grep -q 'Host key verification failed' "$log"; then echo host-key
            elif grep -qiE 'permission denied|authentication failed|could not read (username|password)' "$log"; then echo authentication
            elif grep -qE 'Remote branch .* not found' "$log"; then echo branch
            else echo remote
            fi
            rm -f -- "$log"
            exit 0
        fi
        rm -f -- "$log"
        touch "$marker.complete"
    fi
    echo cloned
    "#,
                            path = quote(path),
                            destination = quote(&destination),
                            marker = quote(&marker),
                            parent = quote(&parent),
                            known = quote(&known),
                            lookup = quote(&lookup),
                            branch = quote(&source.base_branch),
                            url = quote(url)
                        );
                        let outcome = self.ssh(request, &script, None).await.map_err(|_| {
                            "clone failed: cannot run the clone on the worker over SSH".to_owned()
                        })?;
                        let outcome = outcome.lines().last().unwrap_or_default().trim();
                        if outcome != "cloned" {
                            return Err(clone_failure(
                                outcome,
                                &destination,
                                &source.base_branch,
                                host.as_ref(),
                                url,
                            ));
                        }
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
                Ok(())
            }.await;
            let active = self.0.setup.lock().unwrap()[name]
                .active_step
                .clone()
                .unwrap_or(step);
            self.population_result(name, &active, result)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(user: Option<&str>, host: &str, port: Option<u16>) -> Option<SshHost> {
        Some(SshHost {
            user: user.map(String::from),
            host: host.into(),
            port,
        })
    }

    #[test]
    fn ssh_remotes_name_their_server_and_others_name_none() {
        assert_eq!(
            ssh_host("git@github.com:acme/widgets.git"),
            host(Some("git"), "github.com", None)
        );
        assert_eq!(
            ssh_host("ssh://git@git.example:2222/acme/widgets.git"),
            host(Some("git"), "git.example", Some(2222))
        );
        assert_eq!(
            ssh_host("git+ssh://git.example/widgets"),
            host(None, "git.example", None)
        );
        for other in [
            "https://github.com/acme/widgets.git",
            "file:///srv/widgets",
            "/srv/widgets",
            "./widgets:old",
            "-oProxyCommand=x:path",
            "git@bad host:path",
        ] {
            assert_eq!(ssh_host(other), None, "{other}");
        }
    }

    #[test]
    fn the_lookup_and_the_command_carry_a_port_only_off_22() {
        let default = host(Some("git"), "github.com", Some(22)).unwrap();
        assert_eq!(default.lookup(), "github.com");
        assert_eq!(default.destination(), "git@github.com");
        let other = host(None, "git.example", Some(2222)).unwrap();
        assert_eq!(other.lookup(), "[git.example]:2222");
        assert_eq!(other.destination(), "git.example -p 2222");
    }

    #[test]
    fn a_host_key_failure_does_not_claim_the_source_has_no_key() {
        let ssh = host(Some("git"), "git.test", None).unwrap();
        let message = clone_failure("host-key", "/w/p", "main", Some(&ssh), "git@git.test:p");
        assert!(!message.contains("this machine has no"), "{message}");
        assert!(message.contains("ssh -T git@git.test"), "{message}");
    }

    #[test]
    fn a_failure_names_the_host_but_never_the_url_credentials() {
        let message = clone_failure(
            "authentication",
            "/w/p",
            "main",
            None,
            "https://user:token@git.example/acme/widgets.git",
        );
        assert!(message.contains("git.example"), "{message}");
        assert!(!message.contains("token"), "{message}");
    }
}
