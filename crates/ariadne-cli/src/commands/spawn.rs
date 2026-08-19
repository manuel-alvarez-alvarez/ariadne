//! `ariadne _spawn <plan>` — become the agent a spawn plan describes.
//!
//! The daemon's side of this is in `ariadne_core::spawn_plan`: tmux is handed a
//! constant-size command so that neither the briefing nor the environment has
//! to fit in a tmux command line. This end reads the plan, applies it, and
//! `exec`s — so the pane's root process is the agent itself and nothing about
//! pane liveness or the scheduler's reading of it changes.
//!
//! Every failure lands on stderr and exits non-zero, which in a pane means the
//! session's console log: the one place somebody debugging a dead pane looks.

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use ariadne_core::spawn_plan::SpawnPlanFile;

/// Read `plan` and replace this process with the agent it names.
///
/// Returns only on failure: a successful `exec` never comes back.
pub fn exec_plan(plan_path: &Path) -> Result<std::convert::Infallible> {
    let raw = std::fs::read_to_string(plan_path)
        .with_context(|| format!("reading the spawn plan {}", plan_path.display()))?;
    let plan = SpawnPlanFile::from_json(&raw)
        .with_context(|| format!("reading the spawn plan {}", plan_path.display()))?;

    let (program, args) = plan
        .argv
        .split_first()
        .expect("from_json rejects an empty argv");
    let mut cmd = Command::new(program);
    cmd.args(args);
    for (key, value) in &plan.env {
        cmd.env(key, value);
    }
    cmd.current_dir(&plan.cwd);
    // `exec` applies the cwd and the environment and then replaces this
    // process; it returns only when that could not be done at all — an agent
    // CLI that is not on PATH, or a cwd that is gone.
    Err(cmd.exec()).with_context(|| {
        format!(
            "starting {program} in {} (from the spawn plan {})",
            plan.cwd.display(),
            plan_path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plan that cannot be read is reported by path: in a pane log that is
    /// the only clue there is.
    #[test]
    fn a_missing_plan_names_the_path_it_looked_for() {
        let err = format!(
            "{:#}",
            exec_plan(Path::new("/nonexistent/spawn.json")).expect_err("no such plan")
        );
        assert!(err.contains("/nonexistent/spawn.json"), "{err}");
    }

    #[test]
    fn a_corrupt_plan_names_the_path_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = format!("{:#}", exec_plan(&path).expect_err("corrupt plan"));
        assert!(err.contains(path.to_str().unwrap()), "{err}");
        assert!(err.contains("not a valid spawn plan"), "{err}");
    }

    /// A plan naming a program that does not exist fails here rather than
    /// leaving a pane that looks alive.
    #[test]
    fn a_program_that_cannot_be_run_is_reported_with_its_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spawn.json");
        let plan = SpawnPlanFile::new(
            vec!["ariadne-no-such-agent-cli".into()],
            vec![],
            dir.path().to_path_buf(),
        );
        std::fs::write(&path, plan.to_json().unwrap()).unwrap();
        let err = format!("{:#}", exec_plan(&path).expect_err("no such program"));
        assert!(err.contains("ariadne-no-such-agent-cli"), "{err}");
        assert!(err.contains(dir.path().to_str().unwrap()), "{err}");
    }
}
