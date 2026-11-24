"""Run one daemon test under strace and split its wall time by child program.

Usage: trace.py IT_BINARY TEST OUT.strace. A fake `gh` call runs from the
execve of its `gh` link to its exit. The script prints the union of those
intervals, and the time from the test start to the first `gh` call.
"""
import collections, re, subprocess, sys, time, os
binary, test, out = sys.argv[1], sys.argv[2], sys.argv[3]
t0 = time.time()
r = subprocess.run(["strace", "-f", "--seccomp-bpf", "-ttt", "-s", "200",
    "-e", "trace=execve", "-e", "signal=none", "-o", out,
    binary, "--exact", test, "--nocapture", "--test-threads=1"],
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
wall = time.time() - t0
start, kind, end = {}, {}, {}
root = None
ex = re.compile(r'^(\d+) ([\d.]+) execve\("([^"]*)", \[(.*?)\]')
xt = re.compile(r'^(\d+) ([\d.]+) \+\+\+ (?:exited|killed)')
for line in open(out):
    m = ex.match(line)
    if m and line.rstrip().endswith("= 0"):
        pid, ts, path, argv = int(m[1]), float(m[2]), m[3], m[4]
        if root is None:
            root = pid; t_start = ts; continue
        name = os.path.basename(path)
        if pid not in start:
            start[pid] = ts
            if name == "gh": k = "gh"
            elif name in ("sh", "bash") and "acp" in argv: k = "acp"
            else: k = name
            # first program wins, except the launcher that became python
            kind[pid] = k
            if "forge-stub.py" in argv: kind[pid] = "gh"
        else:
            if "forge-stub.py" in argv: kind[pid] = "gh"
            elif kind[pid] in ("sh", "bash", "env"): kind[pid] = name if name != "python3" else ("acp" if "acp" in argv or "agent" in argv else "python3")
        continue
    m = xt.match(line)
    if m:
        pid, ts = int(m[1]), float(m[2])
        if pid == root: t_end = ts
        elif pid in start: end[pid] = ts
def union(iv):
    iv = sorted(iv); tot = 0; cur = None
    for a, b in iv:
        if cur is None or a > cur[1]:
            if cur: tot += cur[1] - cur[0]
            cur = [a, b]
        else: cur[1] = max(cur[1], b)
    if cur: tot += cur[1] - cur[0]
    return tot
by = collections.defaultdict(list)
for pid, a in start.items():
    b = end.get(pid, t_end)
    by[kind[pid]].append((a, b))
allbusy = union([x for v in by.values() for x in v])
print(f"test={test} rc={r.returncode} wall={t_end - t_start:.3f}s busy_any_child={allbusy:.3f}s")
for k, v in sorted(by.items(), key=lambda kv: -union(kv[1])):
    d = [b - a for a, b in v]
    print(f"  {k:14s} n={len(v):4d} union={union(v):7.3f}s sum={sum(d):7.3f}s mean={sum(d)/len(d)*1000:7.1f}ms")
gh = sorted(by["gh"])
first_gh = gh[0][0] - t_start if gh else float("nan")
ghu = union(by["gh"])
print(f"  phases: fixture(start->first gh)={first_gh:.3f}s gh_union={ghu:.3f}s rest(daemon+acp+test)={t_end - t_start - first_gh - ghu:.3f}s gh_share={ghu/(t_end-t_start)*100:.0f}%")
