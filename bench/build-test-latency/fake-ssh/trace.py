"""Run one daemon test with traced children and split its wall time (macOS).

Usage: trace.py IT_BINARY TEST WORK_DIR. WORK_DIR must not exist.

SIP blocks dtrace on macOS, so the script traces with wrap.c:

1. It runs the test once with TMPDIR=WORK_DIR/tmp, so the harness writes its
   shared launchers (`shared_script`) there.
2. It puts the wrapper in the place of each launcher, and of `git` and `tmux`
   on the PATH.
3. It runs the test again, with TRACE_LOG set, and reads one line per child.

The fake `ssh` time is the union of the intervals of the `ssh` children: the
launcher, `python3` and every program that the fake starts, until its exit.
The other children are the other launchers (the ACP agent stub, the fake
`gh`), `git` and `tmux`, outside the `ssh` time. The fixture time runs from the
test start to the first `ssh` call. The rest is the daemon work and the test
code. The wrapper adds about one fork for each traced child.
"""
import collections
import os
import shutil
import subprocess
import sys
import time

binary, test, work = sys.argv[1], sys.argv[2], sys.argv[3]
here = os.path.dirname(os.path.abspath(__file__))
os.makedirs(os.path.join(work, "tmp"))
os.makedirs(os.path.join(work, "bin"))
wrapper = os.path.join(work, "wrap")
subprocess.run(["cc", "-O2", "-o", wrapper, os.path.join(here, "wrap.c")], check=True)
env = dict(os.environ, TMPDIR=os.path.join(work, "tmp"))
cmd = [binary, "--exact", test, "--test-threads=1"]
subprocess.run(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
               timeout=300, check=True)

scripts = os.path.join(work, "tmp", "ariadne-test-scripts")
for name in os.listdir(scripts):
    if "." not in name:
        path = os.path.join(scripts, name)
        os.rename(path, path + ".orig")
        shutil.copy(wrapper, path)
for tool in ("git", "tmux"):
    real = shutil.which(tool)
    if real:
        link = os.path.join(work, "bin", tool)
        shutil.copy(wrapper, link)
        os.symlink(real, link + ".orig")

log = os.path.join(work, "trace.log")
env.update(TRACE_LOG=log, PATH=os.path.join(work, "bin") + os.pathsep + env["PATH"])
t_start = time.time() * 1e6
code = subprocess.run(cmd, env=env, stdout=subprocess.DEVNULL,
                      stderr=subprocess.DEVNULL, timeout=300).returncode
t_end = time.time() * 1e6
time.sleep(0.5)  # let the last watchers write


def union(spans):
    total, cur = 0.0, None
    for a, b in sorted(spans):
        if cur is None or a > cur[1]:
            if cur:
                total += cur[1] - cur[0]
            cur = [a, b]
        else:
            cur[1] = max(cur[1], b)
    if cur:
        total += cur[1] - cur[0]
    return total


by = collections.defaultdict(list)
for line in open(log):
    _pid, a, b, name = line.split(maxsplit=3)
    by[name.strip()].append((float(a), min(float(b), t_end)))
ssh = by.get("ssh", [])
wall = t_end - t_start
ssh_union = union(ssh)
first = min(a for a, _ in ssh)
first_ssh = first - t_start
# Children of the fixture phase count in the fixture time only.
others = [(max(a, first), b) for k, v in by.items() if k != "ssh"
          for a, b in v if b > first]
children_union = union(ssh + others)
print(f"test={test} rc={code} wall={wall / 1e6:.3f}s")
for k, v in sorted(by.items(), key=lambda kv: -union(kv[1])):
    d = [b - a for a, b in v]
    print(f"  {k:14s} n={len(v):4d} union={union(v) / 1e6:7.3f}s "
          f"sum={sum(d) / 1e6:7.3f}s mean={sum(d) / len(d) / 1e3:7.1f}ms")
other = children_union - ssh_union
rest = wall - first_ssh - ssh_union - other
print(f"  phases: fixture={first_ssh / 1e6:.3f}s ssh={ssh_union / 1e6:.3f}s "
      f"other_children={other / 1e6:.3f}s rest={rest / 1e6:.3f}s "
      f"ssh_share={ssh_union / wall * 100:.0f}%")
