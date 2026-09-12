//! The agents a session can run on, and the environment the daemon launches
//! them from.
//!
//! Which agents there are is the daemon's ACP registry, and whether each one
//! can run a session is what discovery measured of it — which is why this
//! half of the report is read off the daemon rather than off this shell.

use ariadne_api::agents::{AcpAgentDto, AcpAgentStatus};
use ariadne_api::doctor::DaemonReportDto;

use super::Check;
use super::checks::{THERE, forge_check, required_tool};

/// The daemon's cached discovery result for each ACP registry entry — and,
/// where none of them is ready, the failure that says no session can be
/// spawned at all.
///
/// Only one ready agent is needed to run sessions, so a rejected one is a
/// warning: it matters only to work pinned to it.
pub fn acp_agents(agents: &[AcpAgentDto]) -> Vec<Check> {
    let mut checks: Vec<Check> = agents
        .iter()
        .map(|agent| match agent.status {
            AcpAgentStatus::Rejected => Check::warn(
                agent.id.clone(),
                format!(
                    "rejected: {}; {}",
                    agent
                        .rejection_reason
                        .as_deref()
                        .unwrap_or("unknown reason"),
                    acp_capabilities(agent)
                ),
            ),
            AcpAgentStatus::Ready if agent.degraded.is_empty() => {
                Check::ok(agent.id.clone(), acp_capabilities(agent))
            }
            AcpAgentStatus::Ready => Check::warn(
                agent.id.clone(),
                format!(
                    "{}; degraded: {}",
                    acp_capabilities(agent),
                    agent
                        .degraded
                        .iter()
                        .map(|gap| match gap {
                            ariadne_api::agents::AcpDegradation::NoEfforts => "no efforts",
                            ariadne_api::agents::AcpDegradation::NoAdoption => "no adoption",
                            ariadne_api::agents::AcpDegradation::NoRestartResume => {
                                "no restart resume"
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        })
        .collect();
    if !agents
        .iter()
        .any(|agent| agent.status == AcpAgentStatus::Ready)
    {
        checks.push(
            Check::fail("any agent", "no ACP agent is ready on the daemon's PATH").hint(
                "install claude-agent-acp, codex-acp or opencode, or add an [[acp_agents]] entry \
                 to config.toml — sessions cannot be spawned without one",
            ),
        );
    }
    checks
}

fn acp_capabilities(agent: &AcpAgentDto) -> String {
    let yes_no = |available| if available { "yes" } else { "no" };
    let capabilities = &agent.capabilities;
    format!(
        "stdio {}; version 1 {}; new {}; model {}; thought level {}; list {}; load {}",
        yes_no(capabilities.stdio),
        yes_no(capabilities.protocol_v1),
        yes_no(capabilities.session_new),
        yes_no(capabilities.model),
        yes_no(capabilities.thought_level),
        yes_no(capabilities.session_list),
        yes_no(capabilities.session_load),
    )
}

/// The daemon's own environment, or the absence of one.
pub fn daemon_environment(daemon: Option<&DaemonReportDto>) -> Vec<Check> {
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

    use ariadne_api::agents::{AcpCapabilitiesDto, AcpDegradation};
    use ariadne_api::doctor::PathStateDto;

    use super::super::Status;
    use super::super::tests::{binary, by_name};

    #[test]
    fn acp_probe_results_show_rejections_and_gaps() {
        let capabilities = AcpCapabilitiesDto {
            stdio: true,
            protocol_v1: true,
            session_new: true,
            model: true,
            thought_level: false,
            session_list: false,
            session_load: false,
        };
        let rejected = AcpAgentDto {
            id: "broken".into(),
            command: vec!["broken".into()],
            builtin: false,
            status: AcpAgentStatus::Rejected,
            capabilities: capabilities.clone(),
            degraded: Vec::new(),
            rejection_reason: Some("model option is missing".into()),
        };
        let degraded = AcpAgentDto {
            id: "limited".into(),
            command: vec!["limited".into()],
            builtin: false,
            status: AcpAgentStatus::Ready,
            capabilities,
            degraded: vec![
                AcpDegradation::NoEfforts,
                AcpDegradation::NoAdoption,
                AcpDegradation::NoRestartResume,
            ],
            rejection_reason: None,
        };

        let checks = acp_agents(&[rejected, degraded]);
        assert!(checks[0].detail.contains("model option is missing"));
        assert!(checks[0].detail.contains("thought level no"));
        assert!(checks[1].detail.contains("no efforts"));
        assert!(checks[1].detail.contains("no adoption"));
        assert!(checks[1].detail.contains("no restart resume"));
    }

    /// A registry with no ready agent is a failure: nothing can be spawned.
    /// One ready agent is enough, and a rejected one beside it is a warning.
    #[test]
    fn no_ready_agent_fails_the_report_and_one_is_enough() {
        let agent = |id: &str, status| AcpAgentDto {
            id: id.into(),
            command: vec![id.into()],
            builtin: true,
            status,
            capabilities: AcpCapabilitiesDto::default(),
            degraded: Vec::new(),
            rejection_reason: None,
        };
        let checks = acp_agents(&[agent("gone", AcpAgentStatus::Rejected)]);
        assert_eq!(by_name(&checks, "any agent").status, Status::Fail);

        let checks = acp_agents(&[
            agent("gone", AcpAgentStatus::Rejected),
            agent("here", AcpAgentStatus::Ready),
        ]);
        assert_eq!(by_name(&checks, "gone").status, Status::Warn);
        assert_eq!(by_name(&checks, "here").status, Status::Ok);
        assert!(checks.iter().all(|check| check.name != "any agent"));
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
            acp_agents: Vec::new(),
            tools: vec![
                binary("git", false, None),
                binary("gh", true, Some(false)),
                binary("glab", false, None),
            ],
            db: there("/home/me/.ariadne/ariadne.db"),
            worktree_root: there("/home/me/.ariadne/worktrees"),
        };
        let checks = daemon_environment(Some(&daemon));
        // Without git the daemon spawns nothing at all: that is a failure. A
        // forge CLI is never one, however it is missing.
        assert_eq!(by_name(&checks, "git").status, Status::Fail);
        assert_eq!(by_name(&checks, "gh").status, Status::Warn);
        assert!(by_name(&checks, "gh").detail.contains("not signed in"));
        let glab = by_name(&checks, "glab");
        assert_eq!(glab.status, Status::Warn);
        assert!(glab.detail.contains("not found on the daemon's PATH"));

        let checks = daemon_environment(None);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Fail);
        assert!(checks[0].hint.is_some());
    }
}
