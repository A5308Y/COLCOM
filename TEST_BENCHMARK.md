# Proposed test runtime comparison

Historical measurement before the save/continue fixes. The failure outcomes below
record what the tests originally caught. Both specs now pass: exact JSON float
parsing preserves clock values, and f64 accumulation preserves room temperatures
when loading rebuilds room stats. The numerical timings have not been remeasured.
The benchmark tool now aborts on any test failure.

5 timed repetitions per case, after warm-up. Default Cargo test profile.
Machine: Darwin 25.6.0, arm64, 16 logical CPUs.
Full-suite times sum direct runs of every test binary built by `cargo test --all-targets`, with the normal parallel test runner.
The baseline adds `--skip regression_tests`; both cases use the same compiled binaries.
All binaries run even when a new regression fails; Cargo itself would stop after the failing library tests.
Compilation is excluded. Library comparisons use the normal parallel runner.
Individual narrow groups run serially and include test-process startup and definition loading.
Their times should not be added to predict the parallel suite time.

The 15 proposed tests comprise two determinism tests, two save/continue tests,
and 11 direct tests across combat, medical work, jobs and battle AI.
Generated-colony tests use seeds 3 and 11; each continuation runs 600 ticks after
saving during an in-flight action. Determinism is checked every 120 ticks through tick 600.

| Case | Median | Min–max | Outcome |
|---|---:|---:|---|
| Existing tests only | 9.699 s | 8.045–12.097 s | Pass |
| Existing + proposed tests | 8.712 s | 5.819–15.767 s | 2 failing regressions |
| Library: existing tests only | 2.088 s | 2.084–2.104 s | Pass |
| Library: existing + proposed tests | 2.121 s | 2.112–2.139 s | 2 failing regressions |
| All proposed tests (parallel) | 0.264 s | 0.261–0.269 s | 2 failing regressions |
| Determinism: generated colonies | 0.229 s | 0.228–0.249 s | Pass |
| Determinism: battle AI | 0.020 s | 0.020–0.021 s | Pass |
| Save/continue: working colonies | 0.261 s | 0.260–0.272 s | 1 failing regressions |
| Save/continue: battle in flight | 0.022 s | 0.022–0.022 s | 1 failing regressions |
| Direct combat tests | 0.006 s | 0.006–0.006 s | Pass |
| Direct medical tests | 0.006 s | 0.006–0.006 s | Pass |
| Direct job tests | 0.006 s | 0.006–0.006 s | Pass |
| Direct battle AI tests | 0.006 s | 0.006–0.006 s | Pass |

Median full-suite difference: **-0.987 s (-10.2%)**.

The full-suite ranges overlap substantially; this does not establish a speedup or a reliable slowdown.

Library-only difference: **+0.033 s (+1.6%)**.

Reproduce with `python3 tools/benchmark_tests.py --repeats 5`.

All additions use the `regression_tests` filter. At the time of this measurement,
existing log-text assertions were unchanged; they have since been replaced with state checks.
No tests are ignored to obtain these results; the two pre-existing ignored tests remain ignored.

## Regressions found

The two save/continue tests originally failed on clock parsing and room-temperature
recounting differences. Both remain enabled and now pass.
Examples: clock `121.13333339244127` becomes `121.13333339244129`; room temperature `6.9162245` becomes `6.916224`.
Both finish all continuation ticks before asserting, so these timings include the intended workload.
The fixes preserve the save format and exact equality assertions. The benchmark
script no longer accepts any test failures.
