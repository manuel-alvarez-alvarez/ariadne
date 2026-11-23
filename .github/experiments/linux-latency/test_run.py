"""Bounded fixtures: never execute the project's benchmark commands."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('latency', Path(__file__).with_name('run.py'))
latency = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(latency)


class Experiment(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / 'source'
        self.source.mkdir()
        self.identity = dict(experiment_id='fixture', run_id='1', run_attempt='1',
                             source_sha=latency.SOURCE, image_os='ubuntu26',
                             image_version='current-image', lane='rust', repetition=1,
                             workflow_sha='a' * 40, packages='fixture', locks={})

    def job(self, commands, **kwargs):
        return latency.run_pair(self.source, self.root / 'private', self.identity,
                                commands, verify=lambda: None, timeout=.3,
                                deadline=time.monotonic() + 10, **kwargs)

    def command(self, text):
        return ('.', [sys.executable, '-c', text])

    def test_cold_warm_replay_retain_only_the_approved_state(self):
        code = """import os,pathlib,json
p=pathlib.Path(os.environ['CARGO_TARGET_DIR']); c=pathlib.Path(os.environ['CARGO_HOME'])
print(json.dumps({'target':(p/'built').exists(),'download':(c/'download').exists(),'target_path':str(p)}))
(p/'built').write_text('built'); (c/'download').write_text('download')
"""
        result = self.job([self.command(code)])
        self.assertEqual([x['phase'] for x in result['commands']], ['cold', 'warm', 'replay'])
        states = [json.loads((Path(result['evidence']) / x['log']).read_text()) for x in result['commands']]
        self.assertEqual([(s['target'], s['download']) for s in states], [(False, False), (True, True), (False, True)])
        self.assertEqual(states[0]['target_path'], states[1]['target_path'])
        self.assertNotEqual(states[1]['target_path'], states[2]['target_path'])
        self.assertTrue((Path(result['evidence']) / 'resources.jsonl').exists())
        self.assertTrue((Path(result['evidence']) / 'result.json').exists())

    def test_failure_preserves_status_and_invalidates_downstream(self):
        result = self.job([self.command("print('first'); raise SystemExit(7)"), self.command("print('must not run')")])
        self.assertEqual(result['commands'][0]['exit_code'], 7)
        self.assertEqual([x['status'] for x in result['commands']], ['failed', 'invalid', 'invalid', 'invalid', 'invalid', 'invalid'])
        self.assertIn('first', (Path(result['evidence']) / 'cold-00.log').read_text())

    def test_timeout_kills_the_whole_process_group(self):
        pidfile = self.root / 'child'
        code = "import subprocess,time,pathlib; p=subprocess.Popen(['sleep','20']); pathlib.Path(%r).write_text(str(p.pid)); time.sleep(20)" % str(pidfile)
        result = self.job([self.command(code)])
        self.assertEqual(result['commands'][0]['exit_code'], 124)
        self.assertEqual(result['commands'][0]['status'], 'timeout')
        pid = int(pidfile.read_text())
        # A killed orphan may briefly remain a zombie on Linux.
        alive = False
        try:
            os.kill(pid, 0)
            proc = Path('/proc') / str(pid) / 'stat'
            alive = not (proc.exists() and proc.read_text().split()[2] == 'Z')
        except ProcessLookupError:
            pass
        self.assertFalse(alive)

    def test_deadline_prevents_new_commands(self):
        result = latency.run_pair(self.source, self.root / 'private', self.identity,
                                 [self.command("raise RuntimeError('not executed')")],
                                 verify=lambda: None, timeout=.2, deadline=time.monotonic() - 1)
        self.assertEqual([x['status'] for x in result['commands']], ['deadline', 'invalid', 'invalid'])

    def test_private_keys_and_environment_disable_inherited_remote_cache(self):
        os.environ['SCCACHE_BUCKET'] = 'unwanted'
        os.environ['RUSTC_WRAPPER'] = 'unwanted'
        self.addCleanup(os.environ.pop, 'SCCACHE_BUCKET')
        self.addCleanup(os.environ.pop, 'RUSTC_WRAPPER')
        code = "import os; print(os.environ.get('SCCACHE_BUCKET')); print(os.environ.get('SCCACHE_GHA_ENABLED')); print(os.environ['CARGO_HOME'])"
        first = self.job([self.command(code)])
        self.identity['repetition'] = 2
        second = self.job([self.command(code)])
        self.assertNotEqual(first['key'], second['key'])
        self.assertTrue((Path(first['evidence']) / 'cold-00.log').read_text().startswith('None\nfalse\n'))
        with self.assertRaises(FileExistsError):
            self.job([self.command(code)])

    def test_images_and_package_manifests_never_pool(self):
        rows = []
        for repetition, image, packages in [(1, 'A', 'p'), (2, 'A', 'p'), (3, 'B', 'p'), (3, 'A', 'q')]:
            identity = dict(self.identity, repetition=repetition, image_version=image, packages=packages)
            rows.append(dict(identity=identity, commands=[dict(phase='cold', index=0, status='ok', wall_seconds=4), dict(phase='warm', index=0, status='ok', wall_seconds=2)]))
        report = latency.summarize(rows)
        self.assertEqual(len(report), 3)
        self.assertEqual(sorted(x['matching_repetitions'] for x in report), [1, 1, 2])
        self.assertTrue(all(x['insufficient_matching_repetitions'] for x in report))
        self.assertEqual(report[0]['paired_warm_cold_ratios'], [0.5, 0.5])

    def test_evidence_limit_records_omissions_and_never_truncates_primary(self):
        result = self.job([self.command("print('primary')")])
        evidence = Path(result['evidence'])
        (evidence / 'oversized.html').write_bytes(b'x' * 1048576)
        latency.enforce_cap(evidence, 65536)
        self.assertTrue((evidence / 'cold-00.log').read_text().startswith('primary'))
        self.assertFalse((evidence / 'oversized.html').exists())
        self.assertIn('oversized.html', (evidence / 'omissions.json').read_text())

    def test_exact_chains_preserve_check_boundaries(self):
        self.assertEqual(len(latency.CHAINS['rust']), 9)
        self.assertEqual(latency.CHAINS['rust'][3][1], ['cargo', 'nextest', 'run', '--workspace', '--profile', 'ci', '--no-run'])
        self.assertEqual(latency.CHAINS['tauri'], [('ui/src-tauri', ['cargo', 'check', '--locked', '--timings'])])
        self.assertEqual(len(latency.CHAINS['frontend']), 12)

    def test_live_primary_cap_stops_a_noisy_command_with_visible_failure(self):
        with mock.patch.object(latency, 'PRIMARY_CAP', 4 * 1024 ** 2 + 131072):
            result = self.job([self.command("print('x' * 1048576)")])
        self.assertEqual(result['commands'][0]['status'], 'evidence_limit')
        self.assertEqual(result['commands'][0]['exit_code'], 125)
        self.assertFalse(result['valid'])
        self.assertLess((Path(result['evidence']) / 'cold-00.log').stat().st_size, 131072)

    def test_phase_source_mismatch_invalidates_later_observations(self):
        verify = mock.Mock(side_effect=[None, RuntimeError('SHA changed')])
        result = latency.run_pair(self.source, self.root / 'private', self.identity,
                                 [self.command("print('cold')")], verify=verify, timeout=.3)
        self.assertEqual([x['status'] for x in result['commands']], ['ok', 'invalid', 'invalid'])
        self.assertIn('SHA changed', result['error'])

    def test_tauri_keeps_the_compiler_wrapper_unset_and_has_no_replay(self):
        self.identity['lane'] = 'tauri'
        result = self.job([self.command("import os; print(os.environ.get('RUSTC_WRAPPER'))")])
        self.assertEqual([r['phase'] for r in result['commands']], ['cold', 'warm'])
        self.assertEqual((Path(result['evidence']) / 'cold-00.log').read_text().strip(), 'None')

    def test_duplicate_guard_reads_every_page_and_catches_a_completed_overlap(self):
        import io
        def response(value):
            return io.BytesIO(json.dumps(value).encode())
        filler = dict(id=1, display_title='other', status='completed', updated_at='2026-11-22T00:00:00Z')
        overlap = dict(id=9, display_title='linux-latency-fixture', status='completed', updated_at='2026-11-23T01:00:00Z')
        replies = [response({'created_at': '2026-11-23T00:00:00Z'}),
                   response({'workflow_runs': [filler] * 100}), response({'workflow_runs': [overlap]})]
        with mock.patch.object(latency.urllib.request, 'urlopen', side_effect=replies) as api:
            with self.assertRaisesRegex(RuntimeError, 'overlapping'):
                latency.duplicate_guard('owner/repository', 'fixture', '10')
            self.assertEqual(api.call_count, 3)
            self.assertIn('page=2', api.call_args.args[0].full_url)
            self.assertIsNone(api.call_args.args[0].get_header('Authorization'))

    def test_duplicate_guard_fails_closed_without_api_access(self):
        with mock.patch.object(latency.urllib.request, 'urlopen', side_effect=OSError('offline')):
            with self.assertRaises(OSError):
                latency.duplicate_guard('owner/repository', 'fixture', '10')
        with self.assertRaises(ValueError):
            latency.validate_id('invalid;command')

    def test_duplicate_repetitions_are_refused_instead_of_replacing_evidence(self):
        row = dict(identity=self.identity, commands=[])
        with self.assertRaisesRegex(ValueError, 'duplicate repetition'):
            latency.summarize([row, row])

    def test_three_matching_images_report_distributions_and_ratios(self):
        rows = [dict(identity=dict(self.identity, repetition=i, setup_seconds=i, memory=str(i)),
                     commands=[dict(phase='cold', index=0, status='ok', wall_seconds=v),
                               dict(phase='warm', index=0, status='ok', wall_seconds=2)])
                for i, v in [(1, 4), (2, 8), (3, 6)]]
        report = latency.summarize(rows)
        self.assertEqual(len(report), 1)
        self.assertFalse(report[0]['insufficient_matching_repetitions'])
        self.assertEqual(report[0]['stages'][0], dict(phase='cold', index=0, n=3, minimum=4, median=6, maximum=8))

    def test_sigterm_preserves_partial_evidence_and_kills_children(self):
        import subprocess
        pidfile = self.root / 'cancel-child'
        module_path = str(Path(__file__).with_name('run.py').resolve())
        code = "import pathlib,time; pathlib.Path(%r).write_text(str(__import__('os').getpid())); time.sleep(20)" % str(pidfile)
        launcher = f"""import importlib.util,json,pathlib,signal,sys,time
spec=importlib.util.spec_from_file_location('latency', {module_path!r})
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
m.run_pair(pathlib.Path({str(self.source)!r}), pathlib.Path({str(self.root / 'private')!r}), {self.identity!r}, [('.', [sys.executable, '-c', {code!r}])], verify=lambda: None, timeout=5)
"""
        process = subprocess.Popen([sys.executable, '-c', launcher], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 3
            while not pidfile.exists() and time.monotonic() < deadline:
                time.sleep(.01)
            self.assertTrue(pidfile.exists())
            process.send_signal(signal.SIGTERM)
            _, stderr = process.communicate(timeout=3)
            self.assertEqual(process.returncode, 0, stderr)
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pidfile.read_text()), 0)
            result = json.loads(next((self.root / 'private').glob('*-evidence/result.json')).read_text())
            self.assertTrue(result['cancelled'])
            self.assertFalse(result['valid'])
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    def test_linker_driver_preserves_arguments_and_records_command_attribution(self):
        spec = importlib.util.spec_from_file_location('linker', Path(__file__).with_name('linker.py'))
        linker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(linker)
        output = str(self.root / 'target/debug/application')
        arguments = ['-o', output, 'object with spaces.o', '-Wl,--as-needed']
        def driver(argv, **kwargs):
            self.assertEqual(argv[5:], ['/usr/bin/cc', *arguments])
            Path(argv[4]).write_text('0.50\t0.20\t0.10\n')
            return type('Exit', (), dict(returncode=0))()
        directory = self.root / 'links'
        directory.mkdir()
        with mock.patch.object(sys, 'argv', ['linker.py', *arguments]), mock.patch.dict(os.environ, LATENCY_LINK_DIR=str(directory), LATENCY_COMMAND='cold-02'), mock.patch.object(linker.subprocess, 'run', side_effect=driver):
            self.assertEqual(linker.main(), 0)
        record = json.loads(next(directory.glob('*.json')).read_text())
        self.assertEqual(record['argv'], arguments)
        self.assertEqual(record['command'], 'cold-02')
        self.assertEqual(record['kind'], 'final-binary')
        self.assertEqual((record['driver_elapsed'], record['user_seconds'], record['system_seconds']), (.5, .2, .1))

    def test_junit_counts_and_runner_summaries_survive_evidence_collection(self):
        (self.source / 'ui').mkdir()
        xml = '<testsuite><testcase name="pass"/><testcase name="fail"><failure/></testcase><testcase name="skip"><skipped/></testcase></testsuite>'
        (self.source / 'ui/junit.xml').write_text(xml)
        evidence = self.root / 'evidence'
        evidence.mkdir()
        target = self.root / 'target'
        target.mkdir()
        latency.collect(self.source, {'CARGO_TARGET_DIR': str(target)}, evidence, 'cold-05', ['npm', 'test'], 'ui')
        result = latency.junit_counts(evidence, 'cold-05')
        self.assertEqual([(r['tests'], r['failures'], r['errors'], r['skipped']) for r in result], [(3, 1, 0, 1)])

    def test_workflow_is_dispatch_only_serial_and_bounded(self):
        import re
        workflow = Path(__file__).parents[2] / 'workflows/linux-latency-experiment.yml'
        text = workflow.read_text()
        triggers = text.split('on:\n', 1)[1].split('permissions:', 1)[0]
        self.assertIn('  workflow_dispatch:', triggers)
        self.assertNotRegex(triggers, r'(?m)^  (push|pull_request|schedule|workflow_run):')
        self.assertIn('  contents: read', text)
        self.assertNotRegex(text, r'(?m)^  (actions|id-token|deployments|packages):')
        self.assertIn('      max-parallel: 1', text)
        self.assertIn('      fail-fast: false', text)
        rows = re.findall(r'lane: (rust|tauri|frontend), repetition: ([123]), minutes: ([0-9]+)', text)
        self.assertEqual(rows, [(lane, str(rep), minutes) for lane, minutes in [('rust', '50'), ('tauri', '30'), ('frontend', '25')] for rep in (1, 2, 3)])
        self.assertEqual(sum(int(row[2]) for row in rows), 315)
        for action in re.findall(r'uses: ([^ \n]+)', text):
            self.assertRegex(action, r'@[a-f0-9]{40}$')
        self.assertIn('retention-days: 14', text)
        self.assertIn('package-manager-cache: false', text)
        self.assertNotRegex(text, r'actions/cache|rust-cache|cache: npm|secrets\.|ACTIONS_RUNTIME_TOKEN|ACTIONS_RESULTS_URL')
        self.assertIn('runs-on: ubuntu-26.04', text)
        self.assertIn('ref: 4f71da2465954107f113d639b5d7bf666c8446a9', text)
        self.assertIn('ref: ${{ github.sha }}', text)

    def test_typescript_inventory_records_state_under_npm_managed_directories(self):
        directory = self.source / 'ui/node_modules/.tmp'
        directory.mkdir(parents=True)
        (directory / 'tsconfig.app.tsbuildinfo').write_text('state')
        self.assertEqual(latency.typescript_state(self.source), [('ui/node_modules/.tmp/tsconfig.app.tsbuildinfo', 5)])


if __name__ == '__main__':
    unittest.main()
