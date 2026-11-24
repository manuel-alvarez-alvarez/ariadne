//! A scriptable stub of `ssh`, as test support (037).
//!
//! [`stub_ssh`] links one shared POSIX `sh` program as `ssh`, as
//! [`super::forge`] links its stub forge CLI: one shared launcher, so no test
//! waits on macOS to check a new executable. Each call stays a child process
//! of its own, but `sh` starts in a few milliseconds where python3 and its
//! imports took about thirty, and the host tests make dozens of calls each.
//! Only a forward starts python3, for the relay that `sh` cannot do.
//!
//! Each run logs its pid and its arguments, then answers from a JSON script,
//! an object with:
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
//! `runtime_bash` in `setup` substitutes the Bash path to model a missing runtime supervisor.
//! `runtime_disconnect` in `setup` names a FIFO. A runtime command closes its stdin
//! when that FIFO reaches EOF, after the test observes an active download.
//!
//! A `setup` script runs the command in a stub login shell. Its `path` is the PATH of
//! `-lc`. `interactive_path` is the PATH of `-ilc`, and `interactive` is `fail`,
//! `unmarked` or `empty` for an interactive shell that fails, prints no markers or prints an empty PATH.
//!
//! [`write_script`] compiles the script into `sh` variables that the stub
//! sources on each call. The forward relay reads the JSON script itself.
//! [`super::HarnessBuilder::ssh`] points the daemon's `ssh_bin` at it.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

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
    /// Make a run end with `pid` in place of the test process.
    pub(crate) fn set_parent_pid(&self, pid: u32) {
        std::fs::write(self._dir.path().join("ssh-parent-pid"), pid.to_string()).unwrap();
    }

    /// Every run, in the order they started.
    pub(crate) fn runs(&self) -> Vec<Run> {
        records(&self.log)
            .into_iter()
            .map(|fields| {
                let [pid, reaper, args @ ..] = &fields[..] else {
                    panic!("an ssh run record holds its pid and its reaper: {fields:?}");
                };
                Run {
                    pid: pid.parse().unwrap(),
                    reaper: (!reaper.is_empty()).then(|| reaper.parse().unwrap()),
                    args: args.to_vec(),
                }
            })
            .collect()
    }
}

/// Write the stub into a directory of its own, answering from `script`.
pub(crate) fn stub_ssh(script: Value) -> StubSsh {
    let dir = tempfile::tempdir().unwrap();
    assert!(script.is_object(), "an ssh script is an object");
    std::fs::write(dir.path().join("ssh-script.json"), script.to_string()).unwrap();
    write_script(&dir.path().join("ssh-script.sh"), &script);
    std::fs::write(
        dir.path().join("ssh-parent-pid"),
        std::process::id().to_string(),
    )
    .unwrap();
    std::fs::write(dir.path().join("ssh-relay.py"), RELAY).unwrap();
    let log = dir.path().join("ssh-runs");
    std::fs::create_dir(&log).unwrap();
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
    let launcher = super::shared_script(STUB);
    let bin = dir.path().join("ssh");
    std::os::unix::fs::symlink(&launcher, &bin).unwrap();
    StubSsh {
        ssh: bin.display().to_string(),
        log,
        _dir: dir,
    }
}

/// Compile `script` into the `sh` variables the stub sources: `s_` for the
/// script, `u_` for its `setup`. Each one holds what the Python stub read
/// from the JSON, with its default, so the stub tests no key itself. A flag
/// is `1` where Python found the value true, and empty where it did not.
fn write_script(path: &Path, script: &Value) {
    let empty = Map::new();
    let script = script.as_object().unwrap();
    let setup = script
        .get("setup")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut sh = String::new();
    // Python refused an environment value with a NUL, and exited 1, where
    // a setup command ran. No `sh` variable holds one, so the stub is told.
    let env_nul = std::cell::Cell::new(false);
    let env = |value: String| {
        if value.contains('\0') {
            env_nul.set(true);
            String::new()
        } else {
            value
        }
    };
    let mut set = |name: &str, value: &str| sh.push_str(&format!("{name}={}\n", quote(value)));
    let text = |map: &Map<String, Value>, key: &str, default: &str| -> String {
        map.get(key).map_or(default.into(), |value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("the ssh script's {key} is a string"))
                .into()
        })
    };
    let flag = |map: &Map<String, Value>, key: &str, default: bool| {
        if map.get(key).map_or(default, truthy) {
            "1"
        } else {
            ""
        }
    };
    let has = |map: &Map<String, Value>, key: &str| if map.contains_key(key) { "1" } else { "" };
    // `os._exit` of the Python stub took the status modulo 256 too.
    let code = |key: &str, default: i64| {
        let code = script.get(key).map_or(default, |code| {
            code.as_i64()
                .unwrap_or_else(|| panic!("the ssh script's {key} is a number"))
        });
        (code & 0xff).to_string()
    };
    set("s_stderr", &text(script, "stderr", ""));
    set("s_exit", &code("exit", 255));
    set(
        "s_signal",
        &script
            .get("signal")
            .filter(|signal| truthy(signal))
            .map_or(String::new(), |signal| {
                signal
                    .as_i64()
                    .expect("the ssh script's signal is a number")
                    .to_string()
            }),
    );
    set("s_forward", flag(script, "forward", false));
    set("s_has_home", has(script, "home"));
    set("s_home", &text(script, "home", ""));
    set("s_home_stderr", &text(script, "home_stderr", ""));
    set("s_home_exit", &code("home_exit", 0));
    set("s_home_hang", flag(script, "home_hang", false));
    let config: String = script
        .get("ssh_config")
        .map(|lines| {
            lines
                .as_array()
                .expect("the ssh script's ssh_config is a list")
                .iter()
                .map(|line| {
                    format!(
                        "{}\n",
                        line.as_str().expect("an ssh_config line is a string")
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    set("s_ssh_config", &config);
    set("s_setup", flag(script, "setup", false));
    set("u_home", &text(setup, "home", ""));
    set("u_has_runtime_bash", has(setup, "runtime_bash"));
    set("u_runtime_bash", &text(setup, "runtime_bash", ""));
    set(
        "u_maintenance_fixture",
        flag(setup, "maintenance_fixture", false),
    );
    set(
        "u_replacement_maintenance",
        flag(setup, "replacement_maintenance", true),
    );
    set(
        "u_worker_maintenance",
        flag(setup, "worker_maintenance", true),
    );
    set("u_has_repository_fixture", has(setup, "repository_fixture"));
    // The registration the stub writes, without its path: the stub ends it
    // with the path of the `repo add` call.
    let registration = setup
        .get("repository_fixture")
        .map_or(String::new(), |fixture| {
            let mut repository = fixture
                .as_object()
                .expect("a repository fixture is an object")
                .clone();
            repository.insert("id".into(), "worker-repository".into());
            repository.remove("path");
            let json = Value::Object(repository).to_string();
            format!("{},", &json[..json.len() - 1])
        });
    set("u_registration", &registration);
    set("u_has_raw_path", has(setup, "raw_path"));
    // A probe PATH may hold a NUL, which no `sh` variable can, so the stub
    // copies it from a file of its own.
    std::fs::write(
        path.with_file_name("ssh-raw-path"),
        text(setup, "raw_path", ""),
    )
    .unwrap();
    set("u_os", &text(setup, "os", "Linux"));
    set("u_arch", &text(setup, "arch", "x86_64"));
    set("u_health_failure", flag(setup, "health_failure", false));
    set("u_has_inherited_path", has(setup, "inherited_path"));
    set("u_inherited_path", &env(text(setup, "inherited_path", "")));
    set("u_path", &env(text(setup, "path", "/usr/bin:/bin")));
    set("u_interactive", &env(text(setup, "interactive", "")));
    set(
        "u_interactive_path",
        &env(text(setup, "interactive_path", "")),
    );
    set(
        "u_shell_fail",
        if flag(setup, "shell_fail", false) == "1" {
            "1"
        } else {
            "0"
        },
    );
    set("u_banner", &env(text(setup, "banner", "")));
    set("u_suffix", &env(text(setup, "suffix", "")));
    set(
        "u_missing_markers",
        if flag(setup, "missing_markers", false) == "1" {
            "1"
        } else {
            "0"
        },
    );
    set("u_fail_service", &env(text(setup, "fail_service", "")));
    set(
        "u_runtime_disconnect",
        &text(setup, "runtime_disconnect", ""),
    );
    set("u_env_nul", if env_nul.get() { "1" } else { "" });
    std::fs::write(path, sh).unwrap();
}

/// Whether Python reads `value` as true.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `text` as one single-quoted `sh` word.
fn quote(text: &str) -> String {
    assert!(!text.contains('\0'), "sh cannot hold a NUL: {text:?}");
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The records the stub wrote into `dir`, in slot order: each slot file
/// holds its field count, then its fields, each ended by a NUL, so an
/// argument may hold any other byte. A slot that is not whole is skipped:
/// its run is still writing it, or the daemon killed the run between the
/// claim and the write, and then it stays empty.
fn records(dir: &Path) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    for slot in 1.. {
        let Ok(bytes) = std::fs::read(dir.join(slot.to_string())) else {
            break;
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut fields: Vec<String> = text.split('\0').map(str::to_string).collect();
        // What follows the last NUL: empty once the record is whole.
        if fields.pop().is_none_or(|rest| !rest.is_empty()) {
            continue;
        }
        let Some(count) = fields.first().and_then(|count| count.parse::<usize>().ok()) else {
            continue;
        };
        if fields.len() != count + 1 {
            continue;
        }
        fields.remove(0);
        records.push(fields);
    }
    records
}

/// What the daemon runs, under the name of the link. `$0` is the link, not
/// the shared file, so the directory of `$0` holds the test's files.
///
/// It answers in the order of the Python stub it replaces: `-G`, then a
/// `setup` command, then the home call, then a forward, which it hands to
/// `ssh-relay.py`, then the end. Each run that does not forward records its
/// pid and its arguments in the next free slot file of `ssh-runs`, created
/// with `set -C` (`O_EXCL`), so concurrent runs never share one.
///
/// A run that waits ends once the test process has ended, as the Python
/// stub did: a hung home call checks it every 0.2 s, and so does the watch
/// beside a read of the `runtime_disconnect` FIFO. Each `sleep` writes to
/// `/dev/null`, so it holds no pipe of the daemon open.
///
/// A `setup` command is parsed as `sh` words with `eval` only where it is a
/// maintenance or a `repo` call, whose words the daemon single-quotes. The
/// Python stub split every command with `shlex` for the `repo` words; this
/// one first looks for ` repo ls` or ` repo add `, so it never runs the words
/// of another command.
const STUB: &str = r#"#!/bin/sh
dir=${0%/*}
IFS= read -r parent < "$dir/ssh-parent-pid"
. "$dir/ssh-script.sh"
alive() {
    kill -0 "$parent" 2>/dev/null
}
pause() {
    sleep "$1" < /dev/null > /dev/null 2>&1
}
slots() {
    set -- "$1"/*
    if [ -e "$1" ]; then slot=$(($# + 1)); else slot=1; fi
}
record() {
    log=$dir/ssh-runs
    slots "$log"
    set -C
    while ! { printf '%s\0' "$#" "$@" > "$log/$slot"; } 2>/dev/null; do
        [ -e "$log/$slot" ] || exit 1
        slot=$((slot + 1))
    done
    set +C
}
G= T= L=
for arg; do
    case $arg in
        -G) G=1;;
        -T) T=1;;
        -L) L=1;;
    esac
done
# A run reaches the forward only past the branches before it.
relay=$s_forward
[ -z "$G" ] || relay=
[ -z "$T" ] || [ -z "$s_setup" ] || relay=
[ -n "$L" ] || [ -z "$s_has_home$s_home_hang" ] || relay=
[ -z "$relay" ] || exec python3 -S "$dir/ssh-relay.py" "$parent" "$dir" "$@"
record "$$" "" "$@"

end() {
    printf '%s' "$s_stderr" >&2
    [ -z "$s_signal" ] || kill -"$s_signal" $$
    exit "$s_exit"
}

if [ -n "$G" ]; then
    printf '%s' "$s_ssh_config"
    exit 0
fi

# What `str.replace` does: every `$2` of `$1` becomes `$3`.
replace() {
    rest=$1
    replaced=
    while :; do
        case $rest in
            *"$2"*)
                replaced=$replaced${rest%%"$2"*}$3
                rest=${rest#*"$2"}
                ;;
            *) break;;
        esac
    done
    replaced=$replaced$rest
}

# Each of these two splits the script that `sh -c` runs into its words,
# as `shlex` did.
maintenance() {
    eval "set -- $command"
    eval "set -- $3"
    while [ $# -gt 0 ] && [ "$1" != --active ]; do shift; done
    if [ $# -eq 0 ]; then
        echo '{"active":false,"ready":false}'
    elif [ "$2" = true ]; then
        : > "$u_home/maintenance-held"
        if [ -e "$u_home/idle" ]; then ready=true; else ready=false; fi
        echo "{\"active\": true, \"ready\": $ready}"
    else
        : > "$u_home/maintenance-resumed"
        echo '{"active":false,"ready":true}'
    fi
}

# Answer a `repo ls` or `repo add` of the repository fixture, or return.
repository() {
    eval "set -- $command"
    eval "set -- $3"
    repo= ls= add= after= path=
    for word; do
        [ -z "$after" ] || { path=$word; after=; }
        case $word in
            repo) repo=1;;
            ls) ls=1;;
            add) [ -n "$add" ] || after=1; add=1;;
        esac
    done
    state=$u_home/worker-repositories.json
    if [ -n "$repo" ] && [ -n "$ls" ]; then
        if [ -e "$state" ]; then cat "$state"; echo; else echo '[]'; fi
        exit 0
    fi
    if [ -n "$repo" ] && [ -n "$add" ]; then
        [ ! -e "$u_home/fail-registration" ] || exit 1
        replace "$path" '\' '\\'
        replace "$replaced" '"' '\"'
        repository=$u_registration'"path": "'$replaced'"}'
        printf '[%s]' "$repository" > "$state"
        printf '%s\n' "$repository"
        exit 0
    fi
}

if [ -n "$T" ] && [ -n "$s_setup" ]; then
    for command; do :; done
    if [ -n "$u_has_runtime_bash" ]; then
        replace "$command" /bin/bash "$u_runtime_bash"
        command=$replaced
    fi
    if [ -n "$u_maintenance_fixture" ]; then
        case $command in
            *--maintenance-protocol*)
                [ -n "$u_replacement_maintenance" ] || exit 2
                echo 1
                exit 0
                ;;
            *"daemon maintenance"*)
                [ -n "$u_worker_maintenance" ] || exit 2
                maintenance
                exit 0
                ;;
        esac
    fi
    if [ -n "$u_has_repository_fixture" ]; then
        case $command in
            *" repo ls"* | *" repo add "*) repository;;
        esac
    fi
    case $command in
        *"uname -s"*)
            if [ -n "$u_has_raw_path" ]; then
                printf '%s\n%s\n%s\n' "$u_os" "$u_arch" "$u_home"
                cat "$dir/ssh-raw-path"
                exit 0
            fi
            ;;
    esac
    case $command in
        *"daemon status"*)
            [ -z "$u_health_failure" ] || exit 1
            exit 0
            ;;
    esac
    [ -z "$u_env_nul" ] || exit 1
    [ -n "$u_has_inherited_path" ] || u_inherited_path=$PATH
    export HOME="$u_home" PATH="$dir/service-bin:$u_inherited_path" SHELL="$dir/login-shell" \
        ARIADNE_SETUP_LOGIN_PATH="$u_path" \
        ARIADNE_SETUP_INTERACTIVE="$u_interactive" \
        ARIADNE_SETUP_INTERACTIVE_PATH="$u_interactive_path" \
        ARIADNE_SETUP_SHELL_FAIL="$u_shell_fail" \
        ARIADNE_SETUP_SHELL_BANNER="$u_banner" \
        ARIADNE_SETUP_SHELL_SUFFIX="$u_suffix" \
        ARIADNE_SETUP_NO_MARKERS="$u_missing_markers" \
        ARIADNE_SETUP_OS="$u_os" \
        ARIADNE_SETUP_ARCH="$u_arch" \
        ARIADNE_SETUP_SERVICE_FAIL="$u_fail_service"
    case $command in
        *"bounded provide"*) [ -z "$u_runtime_disconnect" ] || {
            # Model remote stdin closing after the test observes an active
            # download: the command reads a pipe that closes at the FIFO's EOF.
            {
                cat "$u_runtime_disconnect" > /dev/null &
                reader=$!
                polls=0
                while kill -0 "$reader" 2>/dev/null; do
                    polls=$((polls + 1))
                    [ $((polls % 10)) -ne 0 ] || alive || kill "$reader" 2>/dev/null
                    pause 0.02
                done
            } < /dev/null | /bin/sh -c "$command"
            exit
        };;
    esac
    /bin/sh -c "$command"
    exit
fi

if [ -z "$L" ] && [ -n "$s_has_home$s_home_hang" ]; then
    printf '%s' "$s_home_stderr" >&2
    if [ -n "$s_home_hang" ]; then
        # Wait until killed, or until the test process has ended.
        while alive; do pause 0.2; done
        exit 0
    fi
    printf '%s' "$s_home"
    exit "$s_home_exit"
fi

end
"#;

/// The forward, which `sh` cannot do: `ssh` hands each run that forwards to
/// this program, with the test process's pid and the stub's directory.
const RELAY: &str = r#"
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

# The record of the sh stub: the field count, then each field, NUL-ended,
# in the next free slot.
fields = [str(os.getpid()), "" if reaper is None else str(reaper)] + args
record = "".join(f + "\0" for f in [str(len(fields))] + fields).encode()
log = os.path.join(dir, "ssh-runs")
slot = len(os.listdir(log)) + 1
while True:
    try:
        fd = os.open(os.path.join(log, str(slot)), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
        break
    except FileExistsError:
        slot += 1
os.write(fd, record)
os.close(fd)

def end():
    sys.stderr.write(script.get("stderr", ""))
    sys.stderr.flush()
    if script.get("signal"):
        os.kill(os.getpid(), script["signal"])
    os._exit(script.get("exit", 255))

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
