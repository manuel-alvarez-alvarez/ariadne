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
//! - `signal`: end with this signal after writing stderr, instead of an exit code.
//! - `home`: a run with no `-L`, which asks for the far `$HOME`, prints this
//!   and exits 0. `home_stderr` and `home_exit` make it write and exit with
//!   those instead, and `home_hang` makes it write `home_stderr`, then wait
//!   until it is killed.
//! - `connect_stderr`: what a forward writes to stderr when its far end refuses.
//! - `hold_reaping`: keep the killed forward as a zombie until this file exists.
//! - `ssh_config`: the lines a `-G` run prints, as `ssh -G` prints the
//!   configuration it resolves. Without it a `-G` run prints nothing.
//!
//! `service_pid` in `setup` is the pid the user service runs, 4242 unless
//! set. `stray_pid` is another daemon that answers on the worker socket
//! until a stop of it, which `stray-stops` under the home lists; with
//! `stray_stays` it outlives that stop.
//! `runtime_bash` in `setup` substitutes the Bash path to model a missing runtime supervisor.
//! `runtime_disconnect` in `setup` names a FIFO. A runtime command closes its stdin
//! when that FIFO reaches EOF, after the test observes an active download.
//!
//! A `setup` script runs the command in a stub login shell. Its `path` is the PATH of
//! `-lc`. `interactive_path` is the PATH of `-ilc`, and `interactive` is `fail`,
//! `unmarked` or `empty` for an interactive shell that fails, prints no markers or prints an empty PATH.
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
    pub reaper: Option<u32>,
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
                    reaper: run["reaper"].as_u64().map(|pid| pid as u32),
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
        "#!/bin/sh\ncase \"$1\" in -lc) ;; -ilc) [ \"$ARIADNE_SETUP_INTERACTIVE\" != fail ] || exit 1;; *) exit 1;; esac\n[ \"$ARIADNE_SETUP_SHELL_FAIL\" != 1 ] || exit 1\nprintf '%s' \"$ARIADNE_SETUP_SHELL_BANNER\"\nif [ \"$ARIADNE_SETUP_NO_MARKERS\" = 1 ]; then printf '%s' \"$ARIADNE_SETUP_LOGIN_PATH\"; exit 0; fi\nPATH=$ARIADNE_SETUP_LOGIN_PATH\nif [ \"$1\" = -ilc ]; then\n    case \"$ARIADNE_SETUP_INTERACTIVE\" in\n        unmarked) printf '%s' \"$ARIADNE_SETUP_INTERACTIVE_PATH\"; exit 0;;\n        empty) PATH=;;\n        *) [ -z \"$ARIADNE_SETUP_INTERACTIVE_PATH\" ] || PATH=$ARIADNE_SETUP_INTERACTIVE_PATH;;\n    esac\nfi\nexport PATH\n/bin/sh -c \"$2\"\nresult=$?\nprintf '%s' \"$ARIADNE_SETUP_SHELL_SUFFIX\"\nexit \"$result\"\n",
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
script = json.load(open(os.path.join(dir, "ssh-script.json")))
reaper = None
if script.get("hold_reaping"):
    reaper = os.getpid()
    child = os.fork()
    if child:
        # Model a reaper outside the forward's group, as init would be.
        os.setpgid(0, 0)
        while not os.path.exists(script["hold_reaping"]):
            try:
                os.kill(parent, 0)
            except OSError:
                try:
                    os.kill(child, 9)
                except OSError:
                    pass
                break
            time.sleep(0.01)
        os.waitpid(child, 0)
        os._exit(0)
threading.Thread(target=orphaned, daemon=True).start()

with open(os.path.join(dir, "ssh-runs.jsonl"), "a") as f:
    f.write(json.dumps({"pid": os.getpid(), "args": args, "reaper": reaper}) + "\n")

def end():
    sys.stderr.write(script.get("stderr", ""))
    sys.stderr.flush()
    if script.get("signal"):
        os.kill(os.getpid(), script["signal"])
    os._exit(script.get("exit", 255))

if "-G" in args:
    for line in script.get("ssh_config", []):
        print(line)
    sys.exit(0)

if "-T" in args and script.get("setup"):
    import subprocess
    setup = script["setup"]
    command = args[-1]
    if "runtime_bash" in setup:
        command = command.replace("/bin/bash", setup["runtime_bash"])
    if setup.get("maintenance_fixture") and "--maintenance-protocol" in command:
        if not setup.get("replacement_maintenance", True):
            sys.exit(2)
        print("1")
        sys.exit(0)
    if setup.get("maintenance_fixture") and "daemon maintenance" in command:
        import shlex
        if not setup.get("worker_maintenance", True):
            sys.exit(2)
        parts = shlex.split(shlex.split(command)[2])
        if "--active" not in parts:
            print('{"active":false,"ready":false}')
        elif parts[parts.index("--active") + 1] == "true":
            open(os.path.join(setup["home"], "maintenance-held"), "w").close()
            print(json.dumps({"active": True, "ready": os.path.exists(os.path.join(setup["home"], "idle"))}))
        else:
            open(os.path.join(setup["home"], "maintenance-resumed"), "w").close()
            print('{"active":false,"ready":true}')
        sys.exit(0)
    if "repository_fixture" in setup:
        import shlex
        parts = shlex.split(shlex.split(command)[2])
        state = os.path.join(setup["home"], "worker-repositories.json")
        if "repo" in parts and "ls" in parts:
            print(open(state).read() if os.path.exists(state) else "[]")
            sys.exit(0)
        if "repo" in parts and "add" in parts:
            if os.path.exists(os.path.join(setup["home"], "fail-registration")):
                sys.exit(1)
            repository = dict(setup["repository_fixture"])
            repository["id"] = "worker-repository"
            repository["path"] = parts[parts.index("add") + 1]
            with open(state, "w") as output:
                json.dump([repository], output)
            print(json.dumps(repository))
            sys.exit(0)
    if "uname -s" in command and "raw_path" in setup:
        print(setup.get("os", "Linux"))
        print(setup.get("arch", "x86_64"))
        print(setup["home"])
        sys.stdout.write(setup["raw_path"])
        sys.exit(0)
    # The user service runs `service_pid`. Whatever answers on the socket
    # is `stray_pid` until a stop of it, and the service's process after it;
    # with `stray_stays` the stray outlives its stop.
    service_pid = setup.get("service_pid", 4242)
    stopped = os.path.join(setup["home"], "stray-stopped")
    if "MainPID" in command or "pid = " in command:
        print(service_pid)
        sys.exit(0)
    if "-o comm=" in command:
        with open(os.path.join(setup["home"], "stray-stops"), "a") as stops:
            stops.write(command.split("pid=", 1)[1].split()[0] + "\n")
        if not setup.get("stray_stays"):
            open(stopped, "w").close()
        sys.exit(0)
    if "daemon status" in command:
        if setup.get("health_failure"):
            sys.exit(1)
        stray = setup.get("stray_pid")
        answering = stray if stray and not os.path.exists(stopped) else service_pid
        print(json.dumps({"status": "ok", "pid": answering}))
        sys.exit(0)
    env = {**os.environ, "HOME": setup["home"],
           "PATH": os.path.join(dir, "service-bin") + os.pathsep + setup.get("inherited_path", os.environ["PATH"]),
           "SHELL": os.path.join(dir, "login-shell"),
           "ARIADNE_SETUP_LOGIN_PATH": setup.get("path", "/usr/bin:/bin"),
           "ARIADNE_SETUP_INTERACTIVE": setup.get("interactive", ""),
           "ARIADNE_SETUP_INTERACTIVE_PATH": setup.get("interactive_path", ""),
           "ARIADNE_SETUP_SHELL_FAIL": "1" if setup.get("shell_fail") else "0",
           "ARIADNE_SETUP_SHELL_BANNER": setup.get("banner", ""),
           "ARIADNE_SETUP_SHELL_SUFFIX": setup.get("suffix", ""),
           "ARIADNE_SETUP_NO_MARKERS": "1" if setup.get("missing_markers") else "0",
           "ARIADNE_SETUP_OS": setup.get("os", "Linux"),
           "ARIADNE_SETUP_ARCH": setup.get("arch", "x86_64"),
           "ARIADNE_SETUP_SERVICE_FAIL": setup.get("fail_service", "")}
    if setup.get("runtime_disconnect") and "bounded provide" in command:
        child = subprocess.Popen(command, shell=True, env=env, stdin=subprocess.PIPE)
        # Model remote stdin closing after the test observes an active download.
        with open(setup["runtime_disconnect"], "rb") as gate:
            gate.read()
        child.stdin.close()
        sys.exit(child.wait())
    sys.exit(subprocess.run(command, shell=True, env=env).returncode)

if "-L" not in args and ("home" in script or script.get("home_hang")):
    sys.stderr.write(script.get("home_stderr", ""))
    sys.stderr.flush()
    if script.get("home_hang"):
        # Block until killed, with no clock.
        threading.Event().wait()
    sys.stdout.write(script.get("home", ""))
    sys.stdout.flush()
    os._exit(script.get("home_exit", 0))

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
            sys.stderr.write(script.get("connect_stderr", ""))
            sys.stderr.flush()
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
