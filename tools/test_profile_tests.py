"""Deterministic checks for the opt-in Linux test profiler."""

import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import profile_tests as profile


ROOT = pathlib.Path(__file__).resolve().parent.parent


class ProfileTests(unittest.TestCase):
    def test_proc_stat_handles_spaces_and_parentheses_and_waited_children(self):
        fields = ["S", "12"] + ["0"] * 20
        for index, value in {11: 100, 12: 25, 13: 50, 14: 25,
                             19: 999, 21: 7}.items():
            fields[index] = str(value)
        stat = profile.parse_stat("42 (a worker (with) spaces) " + " ".join(fields),
                                  ticks=100, page_size=4096)
        self.assertEqual(stat["pid"], 42)
        self.assertEqual(stat["ppid"], 12)
        self.assertEqual(stat["name"], "a worker (with) spaces")
        self.assertEqual(stat["start_ticks"], 999)
        self.assertEqual(stat["cpu_seconds"], 2)
        self.assertEqual(stat["rss_bytes"], 7 * 4096)

    def test_descendants_exclude_unrelated_and_reparented_processes(self):
        processes = {1: {"ppid": 0}, 2: {"ppid": 1}, 3: {"ppid": 2},
                     4: {"ppid": 0}, 5: {"ppid": 4}}
        self.assertEqual(profile.descendants(2, processes), {2, 3})
        self.assertEqual(profile.descendants(99, processes), set())

    def test_cpu_arithmetic_uses_effective_capacity_not_percent_of_one_core(self):
        metrics = profile.cpu_metrics(10, 15, 5, 8)
        self.assertEqual(metrics["cpu_seconds"], 20)
        self.assertEqual(metrics["average_cores"], 2)
        self.assertEqual(metrics["capacity_percent"], 25)
        self.assertEqual(profile.cpu_metrics(0, 0, 0, 8)["average_cores"], 0)

    def test_capacity_respects_inherited_cgroup_quota_and_affinity(self):
        def contents(path):
            if str(path) == "/proc/self/cgroup":
                return "0::/parent/child\n"
            if str(path) == "/sys/fs/cgroup/parent/cpu.max":
                return "200000 100000\n"
            if str(path) == "/sys/fs/cgroup/cpu.max":
                raise FileNotFoundError(str(path))
            return "max 100000\n"

        with mock.patch.object(os, "sched_getaffinity", return_value=set(range(8))), \
                mock.patch.object(pathlib.Path, "read_text", autospec=True, side_effect=contents):
            capacity, warnings = profile.effective_capacity()
        self.assertEqual(capacity["effective_cores"], 2)
        self.assertEqual(capacity["affinity_cpus"], 8)
        self.assertEqual(warnings, [])

    def test_completed_events_keep_failures_and_reject_invalid_times(self):
        event = {"type": "test", "event": "failed", "name": "crate::binary$case",
                 "exec_time": 1.25}
        self.assertEqual(profile.test_result(event),
                         {"name": event["name"], "event": "failed", "seconds": 1.25})
        self.assertIsNone(profile.test_result({"type": "suite", "event": "ok"}))
        for value in [-1, float("nan"), True, "2"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                profile.test_result({**event, "exec_time": value})

    def test_inaccessible_quota_is_reported_instead_of_silent_affinity_fallback(self):
        def contents(path):
            if str(path) == "/proc/self/cgroup":
                return "0::/hidden/child\n"
            raise FileNotFoundError(str(path))

        with mock.patch.object(os, "sched_getaffinity", return_value={0, 1}), \
                mock.patch.object(pathlib.Path, "read_text", autospec=True, side_effect=contents):
            capacity, warnings = profile.effective_capacity()
        self.assertEqual(capacity["effective_cores"], 2)
        self.assertTrue(warnings)
        self.assertTrue(any("quota" in warning for warning in warnings))

    def test_low_cpu_intervals_are_elapsed_time_not_sample_counts(self):
        samples = [{"elapsed_seconds": t, "cpu_seconds": cpu}
                   for t, cpu in [(0, 0), (2, 0.1), (5, 0.2), (6, 1.2), (8, 1.2)]]
        self.assertEqual(profile.low_cpu_intervals(samples),
                         [{"start_seconds": 0, "end_seconds": 5, "seconds": 5},
                          {"start_seconds": 6, "end_seconds": 8, "seconds": 2}])

    def test_cpu_tracker_retains_namespace_work_when_parent_accounting_loses_it(self):
        def process(pid, ppid, own, waited=0, start=1):
            return {"pid": pid, "ppid": ppid, "start_ticks": start,
                    "self_cpu_seconds": own, "waited_child_cpu_seconds": waited}

        tracker = profile.CpuTracker(1)
        self.assertAlmostEqual(
            tracker.update([process(1, 0, 0.1), process(2, 1, 0.2, 5),
                            process(3, 2, 0.3)]), 5.6)
        self.assertAlmostEqual(tracker.update([process(1, 0, 0.1, 0.2)]), 5.6)
        self.assertAlmostEqual(tracker.update([process(1, 0, 0.2, 8)]), 8.2)
        self.assertAlmostEqual(
            tracker.update([process(1, 0, 0.2, 8), process(2, 1, 1, start=99)]), 9.2)
        self.assertAlmostEqual(tracker.update([], waited_total=10), 10)

    def test_cpu_tracker_does_not_count_reaped_observed_children_twice(self):
        tracker = profile.CpuTracker(1)
        root = {"pid": 1, "ppid": 0, "start_ticks": 1,
                "self_cpu_seconds": 0.1, "waited_child_cpu_seconds": 0}
        child = {"pid": 2, "ppid": 1, "start_ticks": 2,
                 "self_cpu_seconds": 1, "waited_child_cpu_seconds": 0}
        self.assertAlmostEqual(tracker.update([root, child]), 1.1)
        self.assertAlmostEqual(
            tracker.update([{**root, "self_cpu_seconds": 0.2,
                             "waited_child_cpu_seconds": 1}]), 1.2)

    def test_cpu_tracker_never_flattens_new_work_behind_lost_waited_accounting(self):
        tracker = profile.CpuTracker(1)
        root = {"pid": 1, "ppid": 0, "start_ticks": 1,
                "self_cpu_seconds": 0.1, "waited_child_cpu_seconds": 5}
        child = {"pid": 2, "ppid": 1, "start_ticks": 2,
                 "self_cpu_seconds": 1, "waited_child_cpu_seconds": 0}
        self.assertAlmostEqual(tracker.update([root, child]), 6.1)
        self.assertAlmostEqual(
            tracker.update([{**root, "self_cpu_seconds": 0.2}]), 6.2)
        self.assertAlmostEqual(
            tracker.update([{**root, "self_cpu_seconds": 0.3}]), 6.3)

    def test_report_is_private_retains_failures_and_measures_real_waited_cpu(self):
        with tempfile.TemporaryDirectory(prefix="kvist-profiler-test-") as directory:
            output = pathlib.Path(directory) / "report"
            command = """import json,time,sys
print(json.dumps({'type':'test','event':'failed','name':'fixture$case','exec_time':0.1}),flush=True)
print('fixture diagnostic',file=sys.stderr)
end=time.process_time()+0.12
while time.process_time()<end:
    pass
time.sleep(0.15)
sys.exit(7)
"""
            result = subprocess.run(
                [sys.executable, str(ROOT / "tools/profile_tests.py"),
                 "--output", str(output), "--", sys.executable, "-c", command],
                capture_output=True, text=True, timeout=10,
            )
            self.assertEqual(result.returncode, 7, result.stderr)
            summary = json.loads((output / "summary.json").read_text())
            self.assertEqual(summary["schema_version"], 1)
            self.assertEqual(summary["exit_code"], 7)
            self.assertGreaterEqual(summary["wall_seconds"], 0.25)
            self.assertGreater(summary["cpu_seconds"], 0.05)
            self.assertGreaterEqual(summary["waited_child_cpu_seconds"], 0.1)
            self.assertGreaterEqual(summary["cpu_seconds"], summary["waited_child_cpu_seconds"])
            self.assertEqual(summary["tests"][0]["event"], "failed")
            self.assertIn("fixture diagnostic", (output / "stderr.log").read_text())
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
            self.assertTrue(all(path.stat().st_mode & 0o077 == 0
                                for path in output.iterdir()))
            self.assertGreaterEqual(len((output / "samples.jsonl").read_text().splitlines()), 2)

    def test_existing_or_in_checkout_output_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory(prefix="kvist-profiler-test-") as directory:
            for output in [pathlib.Path(directory), ROOT / "target/profiler-forbidden"]:
                result = subprocess.run(
                    [sys.executable, str(ROOT / "tools/profile_tests.py"),
                     "--output", str(output), "--", "/usr/bin/true"],
                    capture_output=True, text=True, timeout=10,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("profile_tests:", result.stderr)
        self.assertFalse((ROOT / "target/profiler-forbidden").exists())

    def test_termination_is_forwarded_to_the_owned_command(self):
        with tempfile.TemporaryDirectory(prefix="kvist-profiler-test-") as directory:
            ready = pathlib.Path(directory) / "ready"
            command = (
                "import os,pathlib,sys,time; "
                "pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)"
            )
            process = subprocess.Popen(
                [sys.executable, str(ROOT / "tools/profile_tests.py"),
                 "--output", str(pathlib.Path(directory) / "report"),
                 "--", sys.executable, "-c", command, str(ready)],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            )
            try:
                deadline = time.monotonic() + 5
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(ready.exists(), "profiled command did not start")
                child = int(ready.read_text())
                process.send_signal(signal.SIGTERM)
                _, stderr = process.communicate(timeout=5)
                self.assertEqual(process.returncode, 128 + signal.SIGTERM, stderr)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child, 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()

    def test_missing_command_is_explicit_not_a_successful_empty_profile(self):
        with tempfile.TemporaryDirectory(prefix="kvist-profiler-test-") as directory:
            output = pathlib.Path(directory) / "report"
            result = subprocess.run(
                [sys.executable, str(ROOT / "tools/profile_tests.py"),
                 "--output", str(output), "--", "/nonexistent/kvist-profiler-command"],
                capture_output=True, text=True, timeout=10,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("profile_tests:", result.stderr)
            summary = json.loads((output / "summary.json").read_text())
            self.assertFalse(summary["profiling_complete"])
            self.assertIn("profiling_error", summary)

    def test_exited_or_interrupted_leader_cannot_leave_ignoring_descendant_running(self):
        for interrupted in [False, True]:
            with self.subTest(interrupted=interrupted), \
                    tempfile.TemporaryDirectory(prefix="kvist-profiler-test-") as directory:
                ready = pathlib.Path(directory) / "ready"
                output = pathlib.Path(directory) / "report"
                descendant = (
                    "import os,pathlib,signal,sys,time; "
                    "signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                    "pathlib.Path(sys.argv[1]).write_text(f'{os.getpid()} {os.getppid()}'); "
                    "time.sleep(60)"
                )
                leader = (
                    "import pathlib,subprocess,sys,time; "
                    "subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]]); "
                    "\nwhile not pathlib.Path(sys.argv[2]).exists(): time.sleep(0.01)\n"
                    + ("time.sleep(60)" if interrupted else "")
                )
                process = subprocess.Popen(
                    [sys.executable, str(ROOT / "tools/profile_tests.py"),
                     "--output", str(output), "--", sys.executable, "-c",
                     leader, descendant, str(ready)],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                )
                owned = []

                def stopped(pid):
                    try:
                        return profile.parse_stat(
                            pathlib.Path(f"/proc/{pid}/stat").read_text(),
                            ticks=100, page_size=4096,
                        )["state"] in {"Z", "X"}
                    except FileNotFoundError:
                        return True

                try:
                    deadline = time.monotonic() + 5
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(ready.exists(), "descendant did not start")
                    owned = [int(value) for value in ready.read_text().split()]
                    if interrupted:
                        process.send_signal(signal.SIGTERM)
                    _, stderr = process.communicate(timeout=10)
                    self.assertEqual(process.returncode, 143 if interrupted else 0, stderr)
                    deadline = time.monotonic() + 1
                    while not stopped(owned[0]) and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(stopped(owned[0]), "owned descendant survived profiler")
                    summary = json.loads((output / "summary.json").read_text())
                    self.assertTrue(summary["profiling_complete"])
                    self.assertTrue(any("process-group" in warning
                                        for warning in summary["warnings"]))
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait()
                    for pid in owned:
                        if not stopped(pid):
                            try:
                                os.kill(pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass


if __name__ == "__main__":
    unittest.main()
