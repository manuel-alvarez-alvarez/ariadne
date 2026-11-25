//! A real `ariadned` process, for the tests about what only the binary
//! does: its signals, its listeners and the files it leaves in its home.

use std::path::{Path, PathBuf};

/// Runs `ariadned` under a python3 parent that kills it if the test process
/// dies, so no daemon outlives the test however that ends.
const WRAPPER: &str = "import os, subprocess, sys, time
parent = os.getppid()
child = subprocess.Popen(sys.argv[1:])
while child.poll() is None:
    if os.getppid() != parent:
        child.kill()
        break
    # Stand-in poll: the pace of the parent check.
    time.sleep(0.2)
";

/// A running `ariadned`. Dropping it kills it, the wrapper with it.
pub(crate) struct Daemon {
    wrapper: std::process::Child,
    pub(crate) pid_file: PathBuf,
}

impl Daemon {
    /// Start `ariadned` on `home`, with `env` beside the narrow `PATH`.
    pub(crate) fn spawn(home: &Path, env: &[(&str, &str)]) -> Self {
        Self {
            wrapper: std::process::Command::new("python3")
                .args(["-c", WRAPPER, env!("CARGO_BIN_EXE_ariadned"), "--home"])
                .arg(home)
                // The test starts no agent. Keep the daemon from probing any
                // coding-agent command the machine happens to have installed;
                // it still has the system `git` and the stub `gh` by absolute
                // path through the home configuration.
                .env("PATH", "/usr/bin:/bin")
                .envs(env.iter().copied())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
            pid_file: ariadne_client::endpoint::pid_file(home),
        }
    }

    pub(crate) fn pid(&self) -> Option<String> {
        std::fs::read_to_string(&self.pid_file)
            .ok()
            .map(|pid| pid.trim().to_string())
    }

    pub(crate) fn signal(&self, signal: &str) -> bool {
        self.pid().is_some_and(|pid| {
            std::process::Command::new("kill")
                .args([signal, &pid])
                .status()
                .unwrap()
                .success()
        })
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.signal("-KILL");
        let _ = self.wrapper.kill();
        let _ = self.wrapper.wait();
    }
}
