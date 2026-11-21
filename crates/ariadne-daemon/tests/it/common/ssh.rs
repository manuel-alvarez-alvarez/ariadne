//! A scriptable stub of `ssh`, as test support (037).
//!
//! [`stub_ssh`] writes one python3 program and links it as `ssh`, as
//! [`super::forge`] links its stub forge CLI: one shared launcher, so no test
//! waits on macOS to check a new executable. Each run logs its pid and its
//! arguments, then reads a JSON script, an object with:
//!
//! - `forward`: listen on the `-L` port and relay each connection to the
//!   `-L` remote, a unix socket path or a TCP `host:port`, as `ssh` does.
//! - `until`: a file whose creation ends the run. Without it a forward runs
//!   until it is killed, and a run that does not forward ends at once.
//! - `stderr` and `exit`: what the run writes and exits with when it ends.
//!
//! [`super::HarnessBuilder::ssh`] points the daemon's `ssh_bin` at it.

use std::path::PathBuf;

use serde_json::Value;

pub(crate) struct StubSsh {
    pub ssh: String,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

/// One run of the stub, as it logged it.
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub pid: u32,
    pub args: Vec<String>,
}

impl StubSsh {
    /// Every run, in the order they started.
    pub(crate) fn runs(&self) -> Vec<Run> {
        let Ok(log) = std::fs::read_to_string(&self.log) else {
            return Vec::new();
        };
        log.lines()
            .map(|line| {
                let run: Value = serde_json::from_str(line).unwrap();
                Run {
                    pid: run["pid"].as_u64().unwrap() as u32,
                    args: serde_json::from_value(run["args"].clone()).unwrap(),
                }
            })
            .collect()
    }
}

/// Write the stub into a directory of its own, answering from `script`.
pub(crate) fn stub_ssh(script: Value) -> StubSsh {
    let dir = tempfile::tempdir().unwrap();
    let script_file = dir.path().join("ssh-script.json");
    assert!(script.is_object(), "an ssh script is an object");
    std::fs::write(&script_file, script.to_string()).unwrap();
    std::fs::write(
        dir.path().join("ssh-parent-pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    std::fs::write(dir.path().join("ssh-stub.py"), STUB).unwrap();
    let service = super::shared_script(
        "#!/bin/sh\ncase ${0##*/} in uname) case $1 in -s) echo \"$ARIADNE_SETUP_OS\";; -m) echo \"$ARIADNE_SETUP_ARCH\";; esac;; *) [ ! -e \"$ARIADNE_SETUP_SERVICE_FAIL\" ] || exit 1; case ${0##*/} in loginctl) echo yes;; esac;; esac\n",
    );
    let service_bin = dir.path().join("service-bin");
    std::fs::create_dir_all(&service_bin).unwrap();
    for tool in ["systemctl", "launchctl", "loginctl", "uname"] {
        std::os::unix::fs::symlink(&service, service_bin.join(tool)).unwrap();
    }
    let shell = super::shared_script(
        "#!/bin/sh\n[ \"$1\" = -lc ] || exit 1\n[ \"$ARIADNE_SETUP_SHELL_FAIL\" != 1 ] || exit 1\nPATH=$ARIADNE_SETUP_LOGIN_PATH\nexport PATH\nexec /bin/sh -c \"$2\"\n",
    );
    std::os::unix::fs::symlink(shell, dir.path().join("login-shell")).unwrap();
    let launcher = super::shared_script(LAUNCHER);
    let bin = dir.path().join("ssh");
    std::os::unix::fs::symlink(&launcher, &bin).unwrap();
    StubSsh {
        ssh: bin.display().to_string(),
        log: dir.path().join("ssh-runs.jsonl"),
        _dir: dir,
    }
}

/// What the daemon runs: the stub beside the link.
const LAUNCHER: &str = "#!/bin/sh\n\
dir=${0%/*}\n\
IFS= read -r parent < \"$dir/ssh-parent-pid\"\n\
exec python3 -S \"$dir/ssh-stub.py\" \"$parent\" \"$dir\" \"$@\"\n";

const STUB: &str = r#"
import json, os, socket, sys, threading, time

parent, dir, args = int(sys.argv[1]), sys.argv[2], sys.argv[3:]
def orphaned():
    # Ends with the test process, however that one ends.
    while True:
        try:
            os.kill(parent, 0)
        except OSError:
            os._exit(0)
        time.sleep(0.2)
threading.Thread(target=orphaned, daemon=True).start()

with open(os.path.join(dir, "ssh-runs.jsonl"), "a") as f:
    f.write(json.dumps({"pid": os.getpid(), "args": args}) + "\n")
script = json.load(open(os.path.join(dir, "ssh-script.json")))

def end():
    sys.stderr.write(script.get("stderr", ""))
    sys.stderr.flush()
    os._exit(script.get("exit", 255))

if "-T" in args and script.get("setup"):
    import subprocess
    setup = script["setup"]
    command = args[-1]
    if "uname -s" in command and "raw_path" in setup:
        print(setup.get("os", "Linux"))
        print(setup.get("arch", "x86_64"))
        print(setup["home"])
        sys.stdout.write(setup["raw_path"])
        sys.exit(0)
    if "daemon status" in command:
        sys.exit(0)
    env = {**os.environ, "HOME": setup["home"],
           "PATH": os.path.join(dir, "service-bin") + os.pathsep + setup.get("inherited_path", os.environ["PATH"]),
           "SHELL": os.path.join(dir, "login-shell"),
           "ARIADNE_SETUP_LOGIN_PATH": setup.get("path", "/usr/bin:/bin"),
           "ARIADNE_SETUP_SHELL_FAIL": "1" if setup.get("shell_fail") else "0",
           "ARIADNE_SETUP_OS": setup.get("os", "Linux"),
           "ARIADNE_SETUP_ARCH": setup.get("arch", "x86_64"),
           "ARIADNE_SETUP_SERVICE_FAIL": setup.get("fail_service", "")}
    sys.exit(subprocess.run(command, shell=True, env=env).returncode)

if not script.get("forward"):
    end()

bind, port, remote = args[args.index("-L") + 1].split(":", 2)
listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind((bind, int(port)))
listener.listen(64)

def target():
    if remote.startswith("/"):
        s = socket.socket(socket.AF_UNIX)
        s.connect(remote)
    else:
        host, _, p = remote.rpartition(":")
        s = socket.create_connection((host, int(p)))
    return s

def pump(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    for s in (src, dst):
        try:
            s.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass

def serve():
    while True:
        conn, _ = listener.accept()
        try:
            far = target()
        except OSError:
            conn.close()
            continue
        threading.Thread(target=pump, args=(conn, far), daemon=True).start()
        threading.Thread(target=pump, args=(far, conn), daemon=True).start()
threading.Thread(target=serve, daemon=True).start()

until = script.get("until")
while not until or not os.path.exists(until):
    time.sleep(0.02)
end()
"#;
