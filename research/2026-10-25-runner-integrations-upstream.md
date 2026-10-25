# Runner integrations and bounded execution: upstream evidence

Review date: 2026-10-25. Barney revision:
[`e201242fec7bc06ba5c8c65bcc98f853681fd31c`](https://github.com/fabian-barney/crap-typescript/tree/e201242fec7bc06ba5c8c65bcc98f853681fd31c).
Follow-up to [the correctness probes](./2026-10-24-barney-correctness-probes.md).
This note separates executed probes from source-derived lifecycle risks.

## Vitest: useful seam, narrower compatibility than the manifest

Barney's helper preserves configuration, enables coverage unless explicitly
disabled, adds the JSON coverage reporter, and attaches a test reporter. That
reporter's `onFinishedReportCoverage` analyzes the resulting file in
`existing-only` mode: no second test execution. Threshold failure sets
`process.exitCode = 2`; analysis failure sets 1. Disabling coverage omits the
reporter entirely, so it also disables this gate.
Source: [adapter](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/vitest/src/index.ts).

The manifest advertises `vitest: ^4.0.0 || ^5.0.0`, but the repository lock uses
**5.0.5**. In Vitest **4.0.0**, `reportCoverage` invokes this callback only on
`WebSocketReporter` instances. Barney's custom instance does not qualify.
The same restriction is present in sampled 4.0.1, 4.0.5, 4.0.10, 4.0.15 and
4.0.18 sources. In **4.1.0** and **5.0.5**, Vitest checks for the callback on
each reporter and awaits it. Thus the prior passing adapter tests establish
the locked version's behavior, not the entire declared peer range. This is
source evidence; the first version introducing the change and a runtime
4.0 compatibility matrix were not established.
Sources: [manifest](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/vitest/package.json),
[lock](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/package-lock.json),
[Vitest 4.0.0 core](https://github.com/vitest-dev/vitest/blob/v4.0.0/packages/vitest/src/node/core.ts),
[4.1.0 core](https://github.com/vitest-dev/vitest/blob/v4.1.0/packages/vitest/src/node/core.ts),
[5.0.5 core](https://github.com/vitest-dev/vitest/blob/v5.0.5/packages/vitest/src/node/core.ts).

The current official reporter documentation exposes `onCoverage` and
`onTestRunEnd`; `onCoverage` precedes coverage-file reporters. Therefore reading
`coverage-final.json` from `onCoverage` is too early. An adapter could serialize
the callback's coverage map into a private temporary file and call our existing
Rust CLI, or use a verified post-file hook on a deliberately bounded version
range. Either choice needs actual runner integration tests.
Sources: [reporter lifecycle](https://vitest.dev/api/advanced/reporters),
[coverage reporter configuration](https://vitest.dev/config/coverage).

## Jest: file completion is a real coordination problem

Barney forces coverage collection, adds JSON/text coverage output, and attaches
a custom reporter. Its `onRunComplete` registers a single `beforeExit` listener;
that listener finalizes analysis. It polls every 100 ms for up to 5 seconds by
default, accepting either a project report or all discovered package reports.
Readiness checks file existence, without run identity or modification checks.
An old report can therefore satisfy readiness. It uses `existing-only` analysis
and sets the exit code on errors or threshold failure.
Sources: [helper](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/jest/src/index.ts),
[reporter](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/jest/src/reporter.ts).

Jest 30.0.0 registers its built-in `CoverageReporter` after custom reporters.
The dispatcher awaits reporters sequentially. An awaited poll for fresh coverage
inside a custom `onRunComplete` would delay the very reporter that writes it.
Jest then queries reporter errors to calculate success. Barney's later
finalization relies on `process.exitCode`, rather than having its error ready
for that calculation.
Sources: [TestScheduler](https://github.com/jestjs/jest/blob/v30.0.0/packages/jest-core/src/TestScheduler.ts),
[ReporterDispatcher](https://github.com/jestjs/jest/blob/v30.0.0/packages/jest-core/src/ReporterDispatcher.ts),
[CoverageReporter](https://github.com/jestjs/jest/blob/v30.0.0/packages/jest-reporters/src/CoverageReporter.ts).

Lifecycle implications are source-derived, not newly reproduced adapter failures:
`beforeExit` waits for an empty event loop, so open handles postpone analysis;
watch sessions do not obtain a gate on every test run, and the one-shot scheduling
flag cannot reset per run. Explicit `process.exit()` skips this event, making
forced exit a risk. Jest documents `getLastError` as its reporter failure
mechanism and offers `--forceExit`; Node documents the event's limitations.
Sources: [Jest custom reporters](https://jestjs.io/docs/configuration#reporters-arraymodulename--modulename-options),
[Jest CLI](https://jestjs.io/docs/cli#--forceexit),
[Node beforeExit](https://nodejs.org/api/process.html#event-beforeexit).

## Timeout: borrow the bound, improve the cleanup

Barney's shared `runCommand` uses argument-based `spawn` without a shell,
ignores stdin, and drains both output streams. Defaults are **300,000 ms** and
**10 MiB retained per stream**. `CRAP_TYPESCRIPT_COMMAND_TIMEOUT_MS=0` disables
the timer; invalid environment values silently select the default. On timeout
it sends `SIGKILL` to the immediate child and rejects immediately. It does not
establish a process group or explicitly terminate descendants. Retention limits
truncate the beginning-preserving capture without stopping the command;
`rejectOnTruncatedOutput` rejects at completion when requested. The normal
coverage executor returns only the exit code and discards captured diagnostics.
Source: [utils.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/utils.ts).

Executed directly against the pinned TypeScript `runCommand` on macOS with
Node **25.6.1**:

| Probe | Result |
|---|---|
| Child spawns a long-lived grandchild with ignored stdio; command timeout 200 ms | Promise rejected on timeout; grandchild still alive according to signal 0. Probe explicitly killed the grandchild afterward. |
| Child emits `abcdef`/`uvwxyz`; retention limit 3 bytes | Exit 0, stdout `abc`, stderr `uvw`, both truncation flags true. |

These probes required no dependency installation or build. They establish
immediate-child cleanup and capture semantics on this host; Windows behavior,
inherited-pipe shutdown and the full upstream suite were not rerun here.

## Recommendation for our CLI

1. Prioritize a coverage-command deadline that includes output draining and
   descendant cleanup. Killing only the shell/package-manager process is not
   enough. Keep this in the Rust execution boundary so every caller benefits.
2. First document and test sequential runner recipes: run coverage once, then
   invoke the CLI against that freshly produced report. Remove or isolate the
   previous artifact before running; command failure must stop analysis. This
   avoids a second test run and both adapters' lifecycle/version constraints.
3. If config-level integration is still useful, start with a Vitest adapter
   invoking the existing binary. Select a verified coverage-completion boundary
   and test the supported minimum and maximum runner versions, disabled coverage,
   failure, watch reruns, custom directories and strict unknown-evidence failure.
4. Defer a Jest reporter until it has a design for post-coverage completion and
   per-run gating. Do not copy `beforeExit` as the general integration contract.

These are design recommendations, not implemented features. No application code
changed in this investigation.
