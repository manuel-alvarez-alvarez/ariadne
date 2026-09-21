//! Process resource limits the daemon needs before it opens stores or starts
//! agents.

use tracing::{info, warn};

/// Raise the soft open-file limit towards its hard limit, up to a practical
/// ceiling.
///
/// macOS reports an unlimited hard limit but refuses values above `OPEN_MAX`.
/// The ceiling stays below that platform limit and leaves ample room for the
/// daemon's stores, streams, agents and repository watches.
pub fn raise_open_file_limit() {
    #[cfg(unix)]
    {
        use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

        const CAP: u64 = 4_096;

        let old = getrlimit(Resource::Nofile);
        let wanted = old.maximum.map_or(CAP, |maximum| maximum.min(CAP));
        if old.current.is_some_and(|current| current < wanted)
            && let Err(error) = setrlimit(
                Resource::Nofile,
                Rlimit {
                    current: Some(wanted),
                    maximum: old.maximum,
                },
            )
        {
            warn!(
                old = ?old.current,
                wanted,
                error = %error,
                "cannot raise the open file descriptor limit"
            );
        }
        let new = getrlimit(Resource::Nofile);
        info!(
            old = ?old.current,
            new = ?new.current,
            "set the open file descriptor limit"
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::Command;

    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    use super::raise_open_file_limit;

    const CHILD: &str = "ARIADNE_RLIMIT_TEST_CHILD";
    const CAP: u64 = 4_096;

    #[test]
    fn daemon_start_raises_its_soft_open_file_limit() {
        if std::env::var_os(CHILD).is_some() {
            let original = getrlimit(Resource::Nofile);
            let low = original.maximum.map_or(64, |maximum| maximum.min(64));
            setrlimit(
                Resource::Nofile,
                Rlimit {
                    current: Some(low),
                    maximum: original.maximum,
                },
            )
            .unwrap();

            raise_open_file_limit();

            let raised = getrlimit(Resource::Nofile);
            let expected = original.maximum.map_or(CAP, |maximum| maximum.min(CAP));
            assert_eq!(raised.current, Some(expected));
            return;
        }

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "resource::tests::daemon_start_raises_its_soft_open_file_limit",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn installer_services_raise_the_open_file_limit() {
        let installer = include_str!("../../../scripts/install.sh");
        assert!(
            installer.contains(
                "<key>SoftResourceLimits</key>\n    \
                 <dict><key>NumberOfFiles</key><integer>4096</integer></dict>"
            ),
            "the launchd service has no descriptor limit"
        );
        assert!(
            installer.contains("LimitNOFILE=4096"),
            "the systemd service has no descriptor limit"
        );
    }
}
