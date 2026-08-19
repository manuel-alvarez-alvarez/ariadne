//! The spawn plan file: how the daemon tells the `ariadne` CLI what to exec.
//!
//! An agent used to ride into tmux as `tmux new-session … -e K=V … -- <argv>`,
//! which puts the whole briefing and every environment variable into one tmux
//! command — and tmux ships a command to its server in a single imsg message
//! capped near 16KB. A 5KB reviewer briefing was enough to make `new-session`
//! fail with "command too long" and take the task down with it.
//!
//! So the argv no longer travels through tmux at all. The daemon writes it
//! here and tmux runs the constant-size `ariadne _spawn <plan>`, which applies
//! the environment, enters `cwd` and `exec`s the argv in place — the agent
//! itself ends up as the pane's root process, exactly as before.
//!
//! Like [`crate::codex_hooks`], this lives in the domain crate because both
//! ends have to agree on it and neither may depend on the other. That is also
//! why it stays deliberately small: a daemon and a CLI from different builds
//! do meet in the wild (the daemon runs for weeks), and the fewer fields there
//! are, the less there is to disagree about. [`VERSION`] catches the rest.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The plan format this build writes and reads.
///
/// Bumped only for a change an older reader would get *wrong* — a new
/// optional field it may ignore is not one. A reader that meets a version it
/// does not know refuses the launch rather than guessing at it.
pub const VERSION: u32 = 1;

/// Everything `ariadne _spawn` needs to become the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnPlanFile {
    /// See [`VERSION`].
    pub version: u32,
    /// The command to exec; `argv[0]` is the program, looked up on `PATH` when
    /// it is a bare name — the same resolution tmux did with it.
    pub argv: Vec<String>,
    /// Environment variables to set before the exec, as ordered pairs: a JSON
    /// object would reorder them and has nothing to say about a repeated key.
    pub env: Vec<(String, String)>,
    /// The directory the agent runs in (worktree, or the repo for a planner).
    pub cwd: PathBuf,
}

/// Why a plan file could not be turned into a launch.
#[derive(Debug, thiserror::Error)]
pub enum SpawnPlanError {
    #[error("not a valid spawn plan: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error(
        "spawn plan version {found} is not supported (this ariadne speaks version \
         {supported}); the ariadne CLI and the ariadned that wrote the plan are \
         different builds"
    )]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("spawn plan has no command to run")]
    EmptyArgv,
}

impl SpawnPlanFile {
    /// A plan stamped with the version this build writes.
    pub fn new(argv: Vec<String>, env: Vec<(String, String)>, cwd: PathBuf) -> Self {
        Self {
            version: VERSION,
            argv,
            env,
            cwd,
        }
    }

    /// Render the plan for writing. Pretty-printed: it stays in the run dir
    /// after the spawn as the record of how the session was launched, and
    /// somebody reads it.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Read a plan, refusing anything this build cannot launch faithfully.
    pub fn from_json(raw: &str) -> Result<Self, SpawnPlanError> {
        let plan: Self = serde_json::from_str(raw)?;
        if plan.version != VERSION {
            return Err(SpawnPlanError::UnsupportedVersion {
                found: plan.version,
                supported: VERSION,
            });
        }
        if plan.argv.is_empty() {
            return Err(SpawnPlanError::EmptyArgv);
        }
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> SpawnPlanFile {
        SpawnPlanFile::new(
            vec![
                "claude".into(),
                "--append-system-prompt".into(),
                // Newlines, quotes and a shell metacharacter: the plan is the
                // reason none of them need escaping anywhere.
                "You are an engineer.\n\n\"$(rm -rf /)\" — don't.".into(),
            ],
            vec![
                ("ARIADNE_SESSION_ID".into(), "01m0".into()),
                ("ARIADNE_SOCKET".into(), "/tmp/a b/ariadne.sock".into()),
            ],
            PathBuf::from("/tmp/work tree"),
        )
    }

    #[test]
    fn a_plan_survives_the_round_trip_verbatim() {
        let plan = plan();
        assert_eq!(
            SpawnPlanFile::from_json(&plan.to_json().unwrap()).unwrap(),
            plan
        );
    }

    /// The size that started all this: a briefing no tmux command could carry.
    #[test]
    fn a_plan_has_no_size_limit() {
        let mut plan = plan();
        plan.argv.push("B".repeat(200_000));
        let back = SpawnPlanFile::from_json(&plan.to_json().unwrap()).unwrap();
        assert_eq!(back.argv.last().unwrap().len(), 200_000);
    }

    #[test]
    fn the_written_shape_is_the_documented_one() {
        let json: serde_json::Value = serde_json::from_str(&plan().to_json().unwrap()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["argv"][0], "claude");
        assert_eq!(json["env"][0][0], "ARIADNE_SESSION_ID");
        assert_eq!(json["env"][0][1], "01m0");
        assert_eq!(json["cwd"], "/tmp/work tree");
    }

    /// A plan from another build is refused by name rather than launched
    /// half-understood: whatever the newer daemon meant by it is not knowable
    /// here, and a wrong agent command is worse than a failed spawn.
    #[test]
    fn a_plan_from_a_future_daemon_is_refused_with_both_versions() {
        let mut plan = plan();
        plan.version = 99;
        let err = SpawnPlanFile::from_json(&plan.to_json().unwrap())
            .expect_err("unsupported version")
            .to_string();
        assert!(err.contains("version 99 is not supported"), "{err}");
        assert!(err.contains("version 1"), "{err}");
    }

    #[test]
    fn a_plan_with_nothing_to_run_is_refused() {
        let mut plan = plan();
        plan.argv.clear();
        assert!(matches!(
            SpawnPlanFile::from_json(&plan.to_json().unwrap()),
            Err(SpawnPlanError::EmptyArgv)
        ));
    }

    #[test]
    fn garbage_is_refused() {
        assert!(matches!(
            SpawnPlanFile::from_json("not json"),
            Err(SpawnPlanError::Malformed(_))
        ));
        assert!(matches!(
            SpawnPlanFile::from_json(r#"{"version":1,"argv":["x"]}"#),
            Err(SpawnPlanError::Malformed(_))
        ));
    }
}
