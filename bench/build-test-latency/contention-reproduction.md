# Contention reproduction

Use the conditions and commands in [the report](contention.md).
The following driver records elapsed time and bounds task-owned subprocess groups.
Save it as `/tmp/f852fr-contention/run.py`.
The original preparation used uncapped monitors, with the same phase cleanup.
The recorded comparison runs use the capped monitors below.

Create the dedicated worktrees before preparation:

```sh
mkdir -p /tmp/f852fr-contention
git worktree add --detach /tmp/f852fr-contention/a e274fe4b1046a146c10bf1591a6da50114c339a3
git worktree add --detach /tmp/f852fr-contention/b e274fe4b1046a146c10bf1591a6da50114c339a3
python3 /tmp/f852fr-contention/run.py prep
```

The recorded UTC cutoff is historical.
A future reproduction needs a new user-approved window and cutoff.
Use a new measurement root to avoid previous artifacts.
The driver itself creates target directories through Cargo.

```python
import subprocess as s, time, os, signal, json, sys, pathlib, datetime
ROOT=pathlib.Path('/tmp/f852fr-contention')
DEADLINE=datetime.datetime(2026,11,22,21,22,30,tzinfo=datetime.timezone.utc).timestamp()
TEST=['cargo','nextest','run','--locked','-p','ariadne-daemon','--profile','ci','--status-level','fail','--final-status-level','fail']
CLIP=['cargo','clippy','--locked','-p','ariadne-daemon','--all-targets','--','-D','warnings']
def stop(p):
 try: os.killpg(p.pid,signal.SIGTERM)
 except ProcessLookupError: pass
 try: p.wait(timeout=3)
 except s.TimeoutExpired:
  try: os.killpg(p.pid,signal.SIGKILL)
  except ProcessLookupError: pass
  p.wait()
def run(label, specs):
 if time.time()>DEADLINE-30: raise SystemExit('deadline: no further runs')
 monitors=[]; children=[]; start=time.monotonic(); stamp=datetime.datetime.now(datetime.timezone.utc).isoformat()
 try:
  for name,cmd in [('top',['top','-l','120','-s','5','-n','12','-o','cpu']),('disk',['iostat','-d','-w','5','-c','120']),('vm',['vm_stat','-c','120','5'])]:
   f=open(ROOT/(label+'.'+name),'w'); p=s.Popen(cmd,stdout=f,stderr=s.STDOUT,start_new_session=True); monitors.append((p,f))
  for seat,target,cmd in specs:
   log=ROOT/(label+'-'+seat+'.log'); f=open(log,'w'); env=os.environ.copy(); env['CARGO_TARGET_DIR']=str(ROOT/target)
   p=s.Popen(['/usr/bin/time','-l']+cmd,cwd=ROOT/seat,env=env,stdout=f,stderr=s.STDOUT,start_new_session=True)
   children.append((p,f,seat,target,cmd,time.monotonic()))
  rows=[]
  # Poll only the foreground subprocess group to record individual completion times.
  pending=list(children)
  while pending:
   for child in pending[:]:
    p,f,seat,target,cmd,t=child
    if p.poll() is not None:
     rows.append(dict(seat=seat,target=target,command=cmd,elapsed=round(time.monotonic()-t,3),exit=p.returncode)); pending.remove(child)
   if time.time()>=DEADLINE or time.monotonic()-start>=570:
    for child in pending: stop(child[0])
    raise RuntimeError('bounded run stopped')
   if pending: time.sleep(.05)
  record=dict(label=label,start=stamp,elapsed=round(time.monotonic()-start,3),runs=rows)
  with open(ROOT/'results.jsonl','a') as f: f.write(json.dumps(record)+'\n')
  print(json.dumps(record),flush=True)
 finally:
  for p,f,*_ in children: stop(p); f.close()
  for p,f in monitors: stop(p); f.close()
mode=sys.argv[1]
if mode=='prep':
 for seat,target in [('a','shared'),('b','shared'),('a','separate-a'),('b','separate-b')]:
  run('prep-'+seat+'-'+target+'-test',[(seat,target,TEST[:3]+['--no-run']+TEST[3:])])
  run('prep-'+seat+'-'+target+'-clippy',[(seat,target,CLIP)])
else:
 kind,layout,schedule,rep=mode.split(':'); cmd=TEST if kind=='test' else CLIP
 targets=['shared','shared'] if layout=='shared' else ['separate-a','separate-b']
 specs=[(seat,target,cmd) for seat,target in zip(['a','b'],targets)]
 if schedule=='seq':
  for spec in specs: run(mode+'-'+spec[0],[spec])
 else: run(mode,specs)

```

## Recorded comparison order

The first round runs sequential comparisons before concurrent comparisons.
The second round reverses that schedule order and target-layout order.
The order is fixed, rather than randomized.

```sh
python3 /tmp/f852fr-contention/run.py test:separate:seq:1
python3 /tmp/f852fr-contention/run.py test:separate:con:1
python3 /tmp/f852fr-contention/run.py test:shared:seq:1
python3 /tmp/f852fr-contention/run.py test:shared:con:1
python3 /tmp/f852fr-contention/run.py clippy:separate:seq:1
python3 /tmp/f852fr-contention/run.py clippy:separate:con:1
python3 /tmp/f852fr-contention/run.py clippy:shared:seq:1
python3 /tmp/f852fr-contention/run.py clippy:shared:con:1
python3 /tmp/f852fr-contention/run.py test:shared:con:2
python3 /tmp/f852fr-contention/run.py test:shared:seq:2
python3 /tmp/f852fr-contention/run.py test:separate:con:2
python3 /tmp/f852fr-contention/run.py test:separate:seq:2
python3 /tmp/f852fr-contention/run.py clippy:shared:con:2
python3 /tmp/f852fr-contention/run.py clippy:shared:seq:2
python3 /tmp/f852fr-contention/run.py clippy:separate:con:2
python3 /tmp/f852fr-contention/run.py clippy:separate:seq:2
```
