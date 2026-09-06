//! The agents, the profiles that run on them, and the environment they are
//! launched from.
//!
//! The one question underneath all of it is [`Availability`]: which agent
//! binaries the process that spawns sessions can launch. That process is the
//! daemon, not this shell, and the two answers differ whenever a service PATH
//! is older than an install — which is what most of the hints here are about.

use std::path::PathBuf;

use ariadne_api::agents::AgentConfigDto;
use ariadne_api::doctor::{BinaryDto, DaemonReportDto};
use ariadne_core::AgentKind;

use super::checks::{HERE, THERE, describe, forge_check, required_tool};
use super::{Availability, Check, Status};
use crate::codex_trust::{self, Trust};

/// Re-run the installer so the service picks up what your PATH already has.
const REINSTALL: &str = "re-run scripts/install.sh so the service picks";

/// The coding agents as this shell sees them, with the flags each is launched
/// with when the daemon could be asked.
pub fn agents(
    agents: &[BinaryDto],
    flags: &[AgentConfigDto],
    available: &Availability,
) -> Vec<Check> {
    let launched_with = |kind: AgentKind| {
        flags
            .iter()
            .find(|c| c.agent_kind == kind)
            .map(|c| match c.extra_flags.is_empty() {
                true => "; no extra flags".to_string(),
                false => format!("; flags: {}", c.extra_flags.join(" ")),
            })
            .unwrap_or_default()
    };
    // Only one agent is needed to run sessions, so a missing one is a warning
    // rather than a failure: it matters only to work pinned to it, and what a
    // goal or a task is pinned to is read on the goal or the task.
    let mut checks: Vec<Check> = AgentKind::ALL
        .iter()
        .zip(agents)
        .map(|(kind, local)| match local.path {
            Some(_) => Check::ok(
                kind.as_str(),
                format!("{}{}", describe(local, HERE), launched_with(*kind)),
            ),
            None => Check::warn(kind.as_str(), describe(local, HERE))
                .hint("not needed unless a goal or a task is pinned to it"),
        })
        .collect();

    if available.has(AgentKind::Codex) {
        let home = codex_trust::codex_home().unwrap_or_else(|| PathBuf::from(".codex"));
        checks.push(codex_hooks(&Trust::read(&home)));
    }

    if available.effective().is_empty() {
        checks.push(
            Check::fail(
                "any agent",
                format!("no coding agent CLI on {}", available.viewpoint()),
            )
            .hint(match available.stale_service_path() {
                true => format!("they are on your PATH but not the daemon's — {REINSTALL} them up"),
                false => {
                    "install claude, codex or opencode — sessions cannot be spawned without one"
                        .to_string()
                }
            }),
        );
    }
    checks
}

/// Whether codex still trusts the hooks every session it spawns declares
/// ([`crate::codex_trust`]): the difference between a codex session that runs
/// and one that sits on the "Hooks need review" prompt.
///
/// A warning rather than a failure: codex is installed, so some work may be
/// pinned to it, but nothing here knows whether any is — an agent's CLI is
/// chosen per goal and per task, and there is no list of them to consult.
fn codex_hooks(trust: &Trust) -> Check {
    let total = ariadne_core::codex_hooks::EVENTS.len();
    let config = trust.config.display();
    if trust.is_complete() {
        return Check::ok(
            "codex hooks",
            format!("all {total} hook events trusted in {config}"),
        );
    }
    let detail = if trust.is_stale() {
        format!(
            "{} of {total} hook events not trusted: {}",
            trust.untrusted.len(),
            trust.untrusted_keys().join(", ")
        )
    } else if trust.config_exists {
        format!("none of the {total} hook events are trusted in {config}")
    } else {
        format!("codex has no config at {config}, so nothing is trusted")
    };
    Check::new("codex hooks", Status::Warn, detail).hint(match trust.is_stale() {
        // The upgrade case: everything else looks right, which is exactly
        // why it needs saying.
        true => concat!(
            "this Ariadne declares hooks your last setup did not — re-run ",
            "`ariadne setup codex-hooks` or codex stops every session on its ",
            "\"Hooks need review\" prompt",
        ),
        false => concat!(
            "run `ariadne setup codex-hooks` — until then codex stops every ",
            "session on its \"Hooks need review\" prompt",
        ),
    })
}

/// The daemon's own environment, or the absence of one.
pub fn daemon_environment(
    daemon: Option<&DaemonReportDto>,
    available: &Availability,
) -> Vec<Check> {
    let Some(daemon) = daemon else {
        return vec![
            Check::fail(
                "daemon environment",
                "not reported — the daemon did not answer",
            )
            .hint("start it (`ariadne daemon start`) and run doctor again"),
        ];
    };

    let mut checks = vec![Check::ok(
        "PATH",
        daemon.path.clone().unwrap_or_else(|| "unset".to_string()),
    )];

    checks.extend(daemon.agents.iter().map(|binary| {
        let name = binary.name.clone();
        let only_here = binary
            .agent_kind
            .is_some_and(|kind| available.only_on_client(kind));
        match (binary.path.is_some(), only_here) {
            (true, _) => Check::ok(name, describe(binary, THERE)),
            (false, true) => Check::warn(
                name.clone(),
                format!("{name} not on the daemon's PATH, though it is on yours"),
            )
            .hint("the service PATH is fixed at install time — re-run scripts/install.sh"),
            (false, false) => Check::warn(name, describe(binary, THERE))
                .hint("not needed unless a goal or a task is pinned to it"),
        }
    }));

    // A forge CLI is judged by the same two questions wherever it is reported
    // from, and neither of them stops a session being spawned.
    checks.extend(daemon.tools.iter().map(|tool| {
        match tool.authenticated.is_some() || matches!(tool.name.as_str(), "gh" | "glab") {
            true => forge_check(tool, THERE),
            false => required_tool(tool, THERE),
        }
    }));

    let db = &daemon.db;
    checks.push(match (db.exists, db.writable) {
        (true, true) => Check::ok("database", db.path.clone()),
        // Not there yet is ordinary: the daemon creates it on its first
        // write, and `writable` already said the directory takes it.
        (false, true) => Check::warn("database", format!("{} does not exist yet", db.path)),
        (true, false) => Check::fail("database", format!("{} is not writable", db.path))
            .hint("the daemon cannot record anything it does"),
        (false, false) => Check::fail("database", format!("{} cannot be created", db.path))
            .hint("the daemon has no database and cannot write one"),
    });

    let root = &daemon.worktree_root;
    checks.push(match (root.exists, root.writable) {
        (true, true) => Check::ok("worktree root", root.path.clone()),
        (false, _) => Check::fail("worktree root", format!("{} does not exist", root.path))
            .hint("task worktrees are created there; check worktree_root in config.toml"),
        (true, false) => Check::fail(
            "worktree root",
            format!("{} is not writable by the daemon", root.path),
        )
        .hint(
            "the daemon needs write and search permission there — check its owner, \
             its mode, and whether the filesystem is read-only",
        ),
    });
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_api::doctor::PathStateDto;

    use super::super::tests::{binary, by_name};

    /// The three agent CLIs in `AgentKind::ALL` order, `found` of them present.
    fn agent_binaries(found: &[AgentKind]) -> Vec<BinaryDto> {
        AgentKind::ALL
            .into_iter()
            .map(|kind| binary(kind.binary(), Some(kind), found.contains(&kind), None))
            .collect()
    }

    /// Availability as the daemon reports it, whatever this shell has.
    /// A trust verdict with `trusted` of the declared events granted.
    fn trust_for(trusted: &[&'static str]) -> Trust {
        let (trusted, untrusted) = ariadne_core::codex_hooks::EVENTS
            .into_iter()
            .partition(|e| trusted.contains(e));
        Trust {
            config: PathBuf::from("/home/me/.codex/config.toml"),
            config_exists: true,
            trusted,
            untrusted,
        }
    }

    /// Untrusted hooks stop codex before its first turn. Nothing here knows
    /// whether any work is pinned to codex — an agent's CLI is chosen per goal
    /// and per task — so it is a warning either way. Trust granted before an
    /// event was declared has to name the events that will not run and the
    /// command that fixes it, since nothing else about the installation looks
    /// wrong; setup skipped altogether reads differently, and a codex that
    /// never ran has no config to have missed it in.
    #[test]
    fn what_codex_trusts_is_reported_in_the_words_that_say_what_to_do() {
        let all: Vec<_> = ariadne_core::codex_hooks::EVENTS.to_vec();
        let complete = codex_hooks(&trust_for(&all));
        assert_eq!(complete.status, Status::Ok);
        assert!(complete.hint.is_none());

        let one = trust_for(&["SessionStart"]);
        assert_eq!(codex_hooks(&one).status, Status::Warn);

        let old: Vec<_> = ariadne_core::codex_hooks::EVENTS
            .into_iter()
            .filter(|e| *e != "PermissionRequest")
            .collect();
        let stale = codex_hooks(&trust_for(&old));
        assert!(stale.detail.contains("permission_request"), "{stale:?}");
        assert!(
            stale
                .hint
                .as_ref()
                .is_some_and(|h| h.contains("ariadne setup codex-hooks"))
        );

        let never_asked = codex_hooks(&trust_for(&[]));
        assert!(
            never_asked.detail.contains("none of the"),
            "{never_asked:?}"
        );
        let never_ran = codex_hooks(&Trust {
            config_exists: false,
            ..trust_for(&[])
        });
        assert!(never_ran.detail.contains("no config"), "{never_ran:?}");
    }

    /// A missing agent nothing is pinned to is a warning: one agent is enough.
    /// No agent at all is a different matter — nothing can be spawned.
    #[test]
    fn a_missing_agent_is_a_warning_until_none_is_left() {
        let some = agent_binaries(&[AgentKind::ClaudeCode]);
        let checks = agents(&some, &[], &Availability::new(None, &some));
        assert_eq!(
            checks.len(),
            3,
            "no summary failure while one agent is there"
        );
        assert_eq!(checks[0].status, Status::Ok);
        assert_eq!(checks[1].status, Status::Warn);
        assert_eq!(checks[2].status, Status::Warn);

        let none = agent_binaries(&[]);
        let checks = agents(&none, &[], &Availability::new(None, &none));
        assert_eq!(checks.last().unwrap().status, Status::Fail);
    }

    /// The same questions asked of the daemon's own environment, which is the
    /// answer that decides: the daemon is what spawns a session and polls a
    /// published request, and its PATH is not this shell's. A daemon that
    /// never answered still gets a section, and it is a failure.
    #[test]
    fn the_daemon_report_is_read_for_its_tools_the_same_way() {
        let there = |path: &str| PathStateDto {
            path: path.into(),
            exists: true,
            writable: true,
        };
        let daemon = DaemonReportDto {
            version: "0.0.0".into(),
            path: Some("/usr/bin".into()),
            home: "/home/me/.ariadne".into(),
            socket_path: "/home/me/.ariadne/ariadned.sock".into(),
            agents: Vec::new(),
            tools: vec![
                binary("git", None, false, None),
                binary("gh", None, true, Some(false)),
                binary("glab", None, false, None),
            ],
            db: there("/home/me/.ariadne/ariadne.db"),
            worktree_root: there("/home/me/.ariadne/worktrees"),
        };
        let checks = daemon_environment(Some(&daemon), &Availability::default());
        // Without git the daemon spawns nothing at all: that is a failure. A
        // forge CLI is never one, however it is missing.
        assert_eq!(by_name(&checks, "git").status, Status::Fail);
        assert_eq!(by_name(&checks, "gh").status, Status::Warn);
        assert!(by_name(&checks, "gh").detail.contains("not signed in"));
        let glab = by_name(&checks, "glab");
        assert_eq!(glab.status, Status::Warn);
        assert!(glab.detail.contains("not found on the daemon's PATH"));

        let checks = daemon_environment(None, &Availability::default());
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(checks[0].hint.is_some());
    }
}
