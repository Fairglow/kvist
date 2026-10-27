#!/usr/bin/env python3
"""Opt-in Linux process-tree profiling; Python standard library only."""

import argparse
import collections
import json
import math
import os
import pathlib
import resource
import signal
import subprocess
import sys
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parent.parent
INTERVAL = 0.5
LOW_CPU_CORES = 0.25


def parse_stat(text, *, ticks, page_size):
    head, tail = text.rsplit(")", 1)
    pid, name = head.split("(", 1)
    fields = tail.split()
    return {
        "pid": int(pid), "name": name, "state": fields[0], "ppid": int(fields[1]),
        "start_ticks": int(fields[19]),
        "threads": int(fields[17]),
        "self_cpu_seconds": sum(int(fields[index]) for index in (11, 12)) / ticks,
        "waited_child_cpu_seconds": sum(int(fields[index]) for index in (13, 14)) / ticks,
        "cpu_seconds": sum(int(fields[index]) for index in (11, 12, 13, 14)) / ticks,
        "rss_bytes": max(0, int(fields[21])) * page_size,
    }


class CpuTracker:
    """Reconcile observed lifetimes and waited CPU without summing them twice."""

    def __init__(self, root):
        self.root, self.root_key = root, None
        self.entries, self.identities = {}, {}
        self.total, self.own_total, self.regressions = 0, 0, 0

    def update(self, processes, waited_total=None):
        current = {(item["pid"], item["start_ticks"]): item for item in processes}
        self.identities.update({item["pid"]: key for key, item in current.items()})
        for key, item in current.items():
            if item["pid"] == self.root:
                self.root_key = key
            entry = self.entries.setdefault(
                key, {"parent": self.identities.get(item["ppid"]), "own": 0, "waited": 0}
            )
            entry["own"] = max(entry["own"], item["self_cpu_seconds"])
            entry["waited"] = max(entry["waited"], item["waited_child_cpu_seconds"])
        children = collections.defaultdict(list)
        for key, entry in self.entries.items():
            children[entry["parent"]].append(key)

        def total(key):
            entry = self.entries[key]
            live, ended = 0, 0
            for child in children[key]:
                if child in current:
                    live += total(child)
                else:
                    ended += total(child)
            # Namespace supervisors can lose descendant rusage on teardown.
            # Retain observed ended subtrees, but do not add them to waited CPU.
            return entry["own"] + max(entry["waited"], ended) + live

        value = max(total(self.root_key) if self.root_key else 0, waited_total or 0)
        if value + 1e-8 < self.total:
            self.regressions += 1
        own_total = sum(entry["own"] for entry in self.entries.values())
        self.total = max(self.total + own_total - self.own_total, value)
        self.own_total = own_total
        return self.total


def descendants(root, processes):
    children = collections.defaultdict(list)
    for pid, process in processes.items():
        children[process["ppid"]].append(pid)
    selected, pending = set(), [root] if root in processes else []
    while pending:
        pid = pending.pop()
        if pid not in selected:
            selected.add(pid)
            pending.extend(children[pid])
    return selected


def cpu_metrics(wall, user, system, capacity):
    cpu = user + system
    cores = cpu / wall if wall else 0
    return {"cpu_seconds": cpu, "average_cores": cores,
            "capacity_percent": 100 * cores / capacity}


def test_result(event):
    if event.get("type") != "test" or event.get("event") not in {"ok", "failed", "ignored"}:
        return None
    name = event.get("name")
    seconds = event.get("exec_time")
    if not isinstance(name, str) or not name:
        raise ValueError("completed test event has no name")
    if seconds is None and event["event"] == "ignored":
        return {"name": name, "event": "ignored", "seconds": None}
    if (type(seconds) not in (int, float) or not math.isfinite(seconds) or seconds < 0):
        raise ValueError("completed test event has invalid exec_time")
    return {"name": name, "event": event["event"], "seconds": seconds}


def low_cpu_intervals(samples):
    intervals, start = [], None
    for before, after in zip(samples, samples[1:]):
        elapsed = after["elapsed_seconds"] - before["elapsed_seconds"]
        cpu = after["cpu_seconds"] - before["cpu_seconds"]
        low = elapsed > 0 and cpu / elapsed < LOW_CPU_CORES
        if low and start is None:
            start = before["elapsed_seconds"]
        if not low and start is not None:
            end = before["elapsed_seconds"]
            intervals.append({"start_seconds": start, "end_seconds": end,
                              "seconds": end - start})
            start = None
    if start is not None:
        end = samples[-1]["elapsed_seconds"]
        intervals.append({"start_seconds": start, "end_seconds": end, "seconds": end - start})
    return intervals


def effective_capacity():
    affinity = len(os.sched_getaffinity(0))
    quotas, warnings = [], []
    membership = pathlib.Path("/proc/self/cgroup").read_text()
    unified = next((line[3:] for line in membership.splitlines() if line.startswith("0::")), None)
    if unified is None:
        warnings.append("cgroup v2 CPU quota unavailable; capacity uses affinity only")
    else:
        base = pathlib.Path("/sys/fs/cgroup")
        node = base / unified.lstrip("/")
        visible = False
        while node.is_relative_to(base):
            limit = node / "cpu.max"
            try:
                quota, period = limit.read_text().split()
                visible = True
                if quota != "max":
                    quotas.append(int(quota) / int(period))
            except FileNotFoundError:
                # The actual controller root is unlimited and has no cpu.max.
                if node != base or not visible:
                    warnings.append(f"CPU quota not visible at {limit}; capacity may be overestimated")
            except (OSError, ValueError) as error:
                warnings.append(f"cannot inspect CPU quota: {error}")
            node = node.parent
    capacity = min([affinity, *quotas])
    if capacity <= 0:
        raise ValueError("effective CPU capacity must be positive")
    return {"affinity_cpus": affinity, "quota_cores": min(quotas) if quotas else None,
            "effective_cores": capacity}, warnings


def pressure():
    result = {}
    for kind in ("cpu", "io", "memory"):
        try:
            result[kind] = {
                line.split()[0]: dict(field.split("=") for field in line.split()[1:])
                for line in pathlib.Path(f"/proc/pressure/{kind}").read_text().splitlines()
            }
        except OSError as error:
            result[kind] = {"unavailable": str(error)}
    return result


def sample(root, started, observed_io, errors):
    processes = {}
    ticks, page_size = os.sysconf("SC_CLK_TCK"), os.sysconf("SC_PAGE_SIZE")
    unreadable = 0
    for path in pathlib.Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            process = parse_stat((path / "stat").read_text(), ticks=ticks, page_size=page_size)
            processes[process["pid"]] = process
        except (FileNotFoundError, ProcessLookupError):
            continue
        except (OSError, ValueError, IndexError):
            unreadable += 1
    selected = []
    for pid in sorted(descendants(root, processes)):
        process = processes[pid]
        path = pathlib.Path("/proc") / str(pid)
        try:
            process["wait_channel"] = (path / "wchan").read_text().strip()
            counters = dict(line.split(": ", 1) for line in (path / "io").read_text().splitlines())
            key = (pid, process["start_ticks"])
            observed_io[key] = (int(counters["read_bytes"]), int(counters["write_bytes"]))
            if process["threads"] > 1:
                waits = collections.Counter()
                for index, task in enumerate((path / "task").iterdir()):
                    if index >= 256:
                        process["thread_scan_truncated"] = True
                        break
                    try:
                        state = (task / "stat").read_text().rsplit(")", 1)[1].split()[0]
                        waits[(state, (task / "wchan").read_text().strip())] += 1
                    except (FileNotFoundError, ProcessLookupError):
                        continue
                process["thread_waits"] = [
                    {"state": state, "wait_channel": channel, "count": count}
                    for (state, channel), count in sorted(waits.items())
                ]
        except (FileNotFoundError, ProcessLookupError):
            process["exited_during_sample"] = True
        except (OSError, ValueError, KeyError) as error:
            process["sampling_error"] = str(error)
            errors.add(str(error))
        selected.append(process)
    return {
        "elapsed_seconds": time.monotonic() - started,
        "cpu_seconds": sum(process["cpu_seconds"] for process in selected),
        "rss_bytes": sum(process["rss_bytes"] for process in selected),
        "observed_storage_read_bytes": sum(value[0] for value in observed_io.values()),
        "observed_storage_write_bytes": sum(value[1] for value in observed_io.values()),
        "unreadable_proc_stats": unreadable, "processes": selected,
    }


def private_file(path):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    return os.fdopen(descriptor, "w", encoding="utf-8")


def report_directory(output):
    protected = (ROOT, pathlib.Path.cwd().resolve())
    parent = output.parent.resolve() if output else pathlib.Path(tempfile.gettempdir()).resolve()
    candidate = parent / output.name if output else parent
    if any(candidate.is_relative_to(root) for root in protected):
        raise ValueError("report directory must be outside the checkout and working directory")
    if output:
        candidate.mkdir(mode=0o700)
        return candidate
    return pathlib.Path(tempfile.mkdtemp(prefix="kvist-test-profile-", dir=parent))


def read_results(path):
    results, errors, non_events = [], [], 0
    with path.open(encoding="utf-8", errors="replace") as source:
        number = 0
        while line := source.readline((2 << 20) + 1):
            number += 1
            if len(line) > 2 << 20:
                errors.append(f"stdout line {number}: event exceeds 2 MiB")
                while not line.endswith("\n"):
                    line = source.readline((2 << 20) + 1)
                    if not line:
                        break
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                non_events += 1
                continue
            if not isinstance(event, dict):
                non_events += 1
                continue
            try:
                result = test_result(event)
                if result:
                    results.append(result)
            except ValueError as error:
                errors.append(f"stdout line {number}: {error}")
    return sorted(results, key=lambda test: test["seconds"] or 0, reverse=True), errors, non_events


def signal_group(pid, signum):
    try:
        os.killpg(pid, signum)
        return True
    except ProcessLookupError:
        return False


def cleanup_group(child, warnings):
    if child.poll() is None:
        signal_group(child.pid, signal.SIGKILL)
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired as error:
            raise OSError(f"owned command PID {child.pid} did not exit after SIGKILL") from error
    if not signal_group(child.pid, signal.SIGTERM):
        return
    warning = "remaining owned process-group members found; requesting termination"
    warnings.append(warning)
    print(f"profile_tests: {warning}", file=sys.stderr)
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if not signal_group(child.pid, 0):
            return
        time.sleep(0.02)
    signal_group(child.pid, signal.SIGKILL)


def profile(command, directory):
    capacity, warnings = effective_capacity()
    # Fail before effects when procfs is absent or not readable.
    parse_stat(pathlib.Path("/proc/self/stat").read_text(),
               ticks=os.sysconf("SC_CLK_TCK"), page_size=os.sysconf("SC_PAGE_SIZE"))
    before_usage, before_pressure = resource.getrusage(resource.RUSAGE_CHILDREN), pressure()
    samples, observed_io, errors = [], {}, set()
    requested_signal, forwarded_at = [], None
    previous_handlers = {}

    def interrupt(signum, _frame):
        requested_signal.append(signum)

    started = time.monotonic()
    with private_file(directory / "stdout.log") as stdout, \
            private_file(directory / "stderr.log") as stderr, \
            private_file(directory / "samples.jsonl") as stream:
        child = None
        try:
            for signum in (signal.SIGINT, signal.SIGTERM):
                previous_handlers[signum] = signal.signal(signum, interrupt)
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                     start_new_session=True)
            cpu = CpuTracker(child.pid)
            next_notice = 10
            while True:
                current = sample(child.pid, started, observed_io, errors)
                status = child.poll()
                waited = None
                if status is not None:
                    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
                    waited = (usage.ru_utime + usage.ru_stime
                              - before_usage.ru_utime - before_usage.ru_stime)
                current["cpu_seconds"] = cpu.update(current["processes"], waited_total=waited)
                samples.append({key: current[key]
                                for key in ("elapsed_seconds", "cpu_seconds", "rss_bytes")})
                stream.write(json.dumps(current) + "\n")
                stream.flush()
                if status is not None:
                    break
                if requested_signal and forwarded_at is None:
                    signal_group(child.pid, requested_signal[0])
                    forwarded_at = time.monotonic()
                if forwarded_at is not None and time.monotonic() - forwarded_at >= 2:
                    signal_group(child.pid, signal.SIGKILL)
                if current["elapsed_seconds"] >= next_notice:
                    baseline = next((item for item in reversed(samples)
                                     if item["elapsed_seconds"] <= next_notice - 10), samples[0])
                    elapsed = current["elapsed_seconds"] - baseline["elapsed_seconds"]
                    cores = (current["cpu_seconds"] - baseline["cpu_seconds"]) / elapsed
                    waits = collections.Counter()
                    for process in current["processes"]:
                        if "thread_waits" in process:
                            for item in process["thread_waits"]:
                                waits[(item["state"], item["wait_channel"])] += item["count"]
                        else:
                            waits[(process["state"], process.get("wait_channel", "unavailable"))] += 1
                    label = "low CPU (not proof of deadlock)" if cores < LOW_CPU_CORES else "activity"
                    print(f"profile_tests: {current['elapsed_seconds']:.1f}s {label}: "
                          f"{cores:.2f} cores, aggregate RSS {current['rss_bytes'] / 2**20:.1f} MiB; "
                          f"thread waits {dict(waits.most_common(4))}", file=sys.stderr)
                    next_notice += 10
                try:
                    child.wait(timeout=INTERVAL)
                except subprocess.TimeoutExpired:
                    pass
        finally:
            try:
                if child is not None:
                    cleanup_group(child, warnings)
            finally:
                for signum, handler in previous_handlers.items():
                    signal.signal(signum, handler)
    wall = time.monotonic() - started
    after_usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    user = after_usage.ru_utime - before_usage.ru_utime
    system = after_usage.ru_stime - before_usage.ru_stime
    tests, event_errors, non_events = read_results(directory / "stdout.log")
    summary = {
        "schema_version": 1, "profiling_complete": True,
        "exit_code": status, "wall_seconds": wall,
        "user_seconds": user, "system_seconds": system,
        "waited_child_cpu_seconds": user + system,
        "cpu_accounting": "sampled lifetime-tree estimate reconciled with waited CPU",
        **cpu_metrics(wall, cpu.total, 0, capacity["effective_cores"]),
        "capacity": capacity,
        "largest_process_peak_rss_kib": after_usage.ru_maxrss,
        "sampled_peak_aggregate_rss_bytes": max(item["rss_bytes"] for item in samples),
        "observed_storage_read_bytes": sum(value[0] for value in observed_io.values()),
        "observed_storage_write_bytes": sum(value[1] for value in observed_io.values()),
        "input_blocks": after_usage.ru_inblock - before_usage.ru_inblock,
        "output_blocks": after_usage.ru_oublock - before_usage.ru_oublock,
        "voluntary_context_switches": after_usage.ru_nvcsw - before_usage.ru_nvcsw,
        "involuntary_context_switches": after_usage.ru_nivcsw - before_usage.ru_nivcsw,
        "host_pressure_before": before_pressure, "host_pressure_after": pressure(),
        "low_cpu_threshold_cores": LOW_CPU_CORES,
        "low_cpu_intervals": low_cpu_intervals(samples),
        "sampling_counter_regressions": cpu.regressions,
        "sampling_errors": sorted(errors), "warnings": warnings,
        "test_event_errors": event_errors, "non_event_stdout_lines": non_events, "tests": tests,
    }
    with private_file(directory / "summary.json") as output:
        json.dump(summary, output, indent=2, allow_nan=False)
        output.write("\n")
    print(f"profile_tests: exit={status}, wall={wall:.3f}s, attributed CPU~{cpu.total:.3f}s "
          f"({summary['average_cores']:.2f} cores; {summary['capacity_percent']:.1f}% of capacity)",
          file=sys.stderr)
    for test in tests:
        if test["event"] == "failed":
            print(f"profile_tests: {test['seconds']}s failed {test['name']}", file=sys.stderr)
    for test in [test for test in tests if test["event"] != "failed"][:5]:
        print(f"profile_tests: {test['seconds']}s {test['event']} {test['name']}", file=sys.stderr)
    if errors or event_errors or warnings:
        print("profile_tests: report contains sampling/event warnings; inspect summary.json",
              file=sys.stderr)
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=pathlib.Path,
                        help="fresh private report directory outside the checkout (default: /tmp)")
    parser.add_argument("command", nargs=argparse.REMAINDER, help="-- PROGRAM ARG ...")
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("an explicit command after -- is required")
    directory = None
    try:
        directory = report_directory(args.output)
        print(f"profile_tests: private reports: {directory}", file=sys.stderr, flush=True)
        status = profile(command, directory)
        return status if status >= 0 else 128 - status
    except (OSError, ValueError, IndexError) as error:
        print(f"profile_tests: {error}", file=sys.stderr)
        if directory is not None and not (directory / "summary.json").exists():
            try:
                with private_file(directory / "summary.json") as output:
                    json.dump({"schema_version": 1, "profiling_complete": False,
                               "exit_code": 1, "profiling_error": str(error)}, output, indent=2)
                    output.write("\n")
            except OSError as report_error:
                print(f"profile_tests: cannot retain failure summary: {report_error}",
                      file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
