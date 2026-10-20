#!/usr/bin/env python3
"""Compare the existing suite and CODE_HEALTH additions without changing source files.

Run from anywhere: python3 tools/benchmark_tests.py --repeats 5
The baseline skips only `regression_tests`, using the same compiled binaries.
Compilation and one warm-up per suite are excluded from the reported timings.
"""

import argparse
import json
import os
import platform
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
KNOWN_FAILURES = {
    "regression_tests::colony_save_and_continue_matches_uninterrupted_work",
    "regression_tests::battle_save_and_continue_matches_uninterrupted_actions",
}


def failures(output):
    if "\nfailures:\n" not in output:
        return set()
    section = output.rsplit("\nfailures:\n", 1)[1].split("\ntest result:", 1)[0]
    return {line.strip() for line in section.splitlines() if line.strip()}


def run(command):
    start = time.perf_counter()
    env = os.environ.copy()
    env["CARGO_MANIFEST_DIR"] = str(ROOT)  # Match Cargo's asset-root environment for direct binary runs.
    result = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    elapsed = time.perf_counter() - start
    failed = failures(result.stdout)
    if result.returncode and not (result.returncode == 101 and failed and failed <= KNOWN_FAILURES):
        log = ROOT / "TEST_BENCHMARK_FAILURE.log"
        log.write_text(result.stdout)
        raise SystemExit(f"Command failed: {' '.join(map(str, command))}\nFull output: {log}")
    return elapsed, result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error("use at least three repetitions")
    print("Building test binaries (excluded from timings)...", flush=True)
    _, output = run(["cargo", "test", "--all-targets", "--no-run", "--message-format=json"])
    artifacts = []
    for line in output.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("executable") and message["profile"]["test"]:
            artifacts.append(message)
    lib = next(m["executable"] for m in artifacts if "lib" in m["target"]["kind"])
    executables = list(dict.fromkeys(m["executable"] for m in artifacts))

    def run_suite(extra):
        elapsed, outputs = 0.0, []
        for executable in executables:
            duration, output = run([executable, "--quiet", *extra])
            elapsed += duration
            outputs.append(output)
        return elapsed, "\n".join(outputs)

    suites = {
        "Existing tests only": ["--skip", "regression_tests"],
        "Existing + proposed tests": [],
    }
    print("Warming both suites...", flush=True)
    for extra in suites.values():
        run_suite(extra)
    measurements = {name: [] for name in suites}
    outcomes = {}
    for repeat in range(args.repeats):
        # Alternate order to reduce systematic effects from temperature and caches.
        order = list(suites) if repeat % 2 == 0 else list(reversed(suites))
        for name in order:
            elapsed, output = run_suite(suites[name])
            failed = failures(output)
            if name in outcomes and outcomes[name] != failed:
                raise SystemExit(f"Inconsistent test outcome across repetitions: {name}")
            outcomes[name] = failed
            measurements[name].append(elapsed)
            print(f"Run {repeat + 1}/{args.repeats}: {name}: {elapsed:.3f} s", flush=True)
    # Front-end asset loading makes full-suite timings noisy. Also isolate the
    # library where every proposed test lives, with its normal parallel runner.
    library = {
        "Library: existing tests only": [lib, "--quiet", "--skip", "regression_tests"],
        "Library: existing + proposed tests": [lib, "--quiet"],
        "All proposed tests (parallel)": [lib, "--quiet", "regression_tests"],
    }
    for name, command in library.items():
        _, output = run(command)
        outcomes[name] = failures(output)
        measurements[name] = []
    for repeat in range(args.repeats):
        order = list(library) if repeat % 2 == 0 else list(reversed(library))
        for name in order:
            elapsed, _ = run(library[name])
            measurements[name].append(elapsed)
    for name in library:
        print(f"{name}: {statistics.median(measurements[name]):.3f} s median", flush=True)
    groups = {
        "Determinism: generated colonies": "regression_tests::seeded_colonies_are_deterministic",
        "Determinism: battle AI": "regression_tests::automated_battles_are_deterministic",
        "Save/continue: working colonies": "regression_tests::colony_save_and_continue_matches_uninterrupted_work",
        "Save/continue: battle in flight": "regression_tests::battle_save_and_continue_matches_uninterrupted_actions",
        "Direct combat tests": "combat::regression_tests::",
        "Direct medical tests": "medical::regression_tests::",
        "Direct job tests": "jobs::regression_tests::",
        "Direct battle AI tests": "battle_ai::regression_tests::",
    }
    for name, test_filter in groups.items():
        command = [lib, test_filter, "--test-threads=1", "--quiet"]
        _, output = run(command)
        outcomes[name] = failures(output)
        measurements[name] = [run(command)[0] for _ in range(args.repeats)]
        print(f"{name}: {statistics.median(measurements[name]):.3f} s median", flush=True)
    old, new = (statistics.median(measurements[name]) for name in suites)
    report = [
        "# Proposed test runtime comparison", "",
        f"{args.repeats} timed repetitions per case, after warm-up. Default Cargo test profile.",
        f"Machine: {platform.system()} {platform.release()}, {platform.machine()}, {os.cpu_count()} logical CPUs.",
        "Full-suite times sum direct runs of every test binary built by `cargo test --all-targets`, with the normal parallel test runner.",
        "The baseline adds `--skip regression_tests`; both cases use the same compiled binaries.",
        "All binaries run even when a new regression fails; Cargo itself would stop after the failing library tests.",
        "Compilation is excluded. Library comparisons use the normal parallel runner.",
        "Individual narrow groups run serially and include test-process startup and definition loading.",
        "Their times should not be added to predict the parallel suite time.", "",
        "The 15 proposed tests comprise two determinism tests, two save/continue tests,",
        "and 11 direct tests across combat, medical work, jobs and battle AI.",
        "Generated-colony tests use seeds 3 and 11; each continuation runs 600 ticks after",
        "saving during an in-flight action. Determinism is checked every 120 ticks through tick 600.", "",
        "| Case | Median | Min–max | Outcome |", "|---|---:|---:|---|",
    ]
    for name, values in measurements.items():
        status = f"{len(outcomes[name])} failing regressions" if outcomes[name] else "Pass"
        report.append(f"| {name} | {statistics.median(values):.3f} s | {min(values):.3f}–{max(values):.3f} s | {status} |")
    lib_old = statistics.median(measurements["Library: existing tests only"])
    lib_new = statistics.median(measurements["Library: existing + proposed tests"])
    report += ["", f"Median full-suite difference: **{new - old:+.3f} s ({100 * (new / old - 1):+.1f}%)**.",
               "The full-suite ranges overlap substantially; this does not establish a speedup or a reliable slowdown.",
               f"Library-only difference: **{lib_new - lib_old:+.3f} s ({100 * (lib_new / lib_old - 1):+.1f}%)**.", "",
               "Reproduce with `python3 tools/benchmark_tests.py --repeats 5`.", "",
               "All additions use the `regression_tests` filter. Existing log-text assertions are unchanged.",
               "No tests are ignored to obtain these results; the two pre-existing ignored tests remain ignored.", "",
               "## Regressions found", "",
               "The two save/continue tests fail on existing serialization precision differences. They remain enabled.",
               "Examples: clock `121.13333339244127` becomes `121.13333339244129`; room temperature `6.9162245` becomes `6.916224`.",
               "Both finish all continuation ticks before asserting, so these timings include the intended workload.",
               "Production code is unchanged. The script accepts only these two known failure names and aborts on any other failure.", ""]
    path = ROOT / "TEST_BENCHMARK.md"
    path.write_text("\n".join(report))
    print(f"Report: {path}", flush=True)


if __name__ == "__main__":
    main()
