# Runner integrations and coverage-command timeout

Review date: 2026-10-25. Local application revision: `dfef61b` (the earlier
research updates were already present as working-tree changes). This is a
follow-up investigation, not an implementation or dependency update.
See the [competitor refresh](./2026-10-24-competitor-refresh.md),
[Barney probes](./2026-10-24-barney-correctness-probes.md), and the separate
[upstream lifecycle investigation](./2026-10-25-runner-integrations-upstream.md).

## Local boundary

The useful integration boundary already exists: supply an Istanbul JSON or
LCOV artifact to the native CLI. The npm package currently exports a command,
not a supported JavaScript analysis API; its launcher selects a bundled native
binary. There is no reason to port scoring to JavaScript to add runner hooks.
Sources: [package manifest](../packages/crap4ts/package.json),
[launcher](../packages/crap4ts/bin/crap4ts.js), [usage](../docs/usage.md).

Generated mode executes argv directly, removes the selected old artifact,
requires command success and a fresh readable artifact, and forwards both child
streams to stderr. Existing-artifact mode is explicit through `--no-generate`.
These are valuable semantics to retain. Each package group has its own command
and root, and groups are preflighted before commands run. Sources:
[generation.rs](../crates/crap4ts-cli/src/generation.rs),
[main.rs](../crates/crap4ts-cli/src/main.rs),
[generation integration tests](../crates/crap4ts-cli/tests/cli.rs).

## Timeout: actual gap and reproduced traps

`generate` blocks in `child.wait()`, then joins both pipe-reader threads.
There is no deadline. Output is already drained concurrently in fixed 16 KiB
buffers; Barney's captured-output cap addresses a different allocation model.
Our total log volume is unlimited, but child logs do not accumulate in a Vec.
Source: [generate and stream_output](../crates/crap4ts-cli/src/generation.rs).

Built the local binary with `cargo build -q -p crap4ts` and ran two temporary
projects using Python's subprocess watchdog, a separate process session, and
explicit process-group cleanup. Each project selected `src/a.ts`, had an old
`coverage.json`, and configured one of these commands:

| Command | Observation at external 1-second deadline |
|---|---|
| `["sh", "-c", "sleep 30"]` | CLI still blocked; old artifact removed |
| `["sh", "-c", "sleep 30 & exit 0"]` | CLI still blocked; old artifact removed |

The second case demonstrates why a deadline around only `child.wait()` is
insufficient: a descendant inherits the pipes after the direct child exits.
The reader joins can still block. These are macOS probes with an external
watchdog, not a test of a proposed timeout implementation or a 300-second wait.

Rust documents that dropping `Child` does not terminate or reap it, and that
`wait` blocks until exit; `try_wait` provides a nonblocking status check.
Explicit termination and reaping are required. Sources:
[Rust Child](https://doc.rust-lang.org/std/process/struct.Child.html),
[Unix process-group setup](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.process_group).

### Proposed contract (not existing options)

- Default 300 seconds per generated command, with `0` explicitly disabling it.
  Expose `--coverage-timeout-ms` and `coverage.timeout_ms`; each group owns its
  setting. Preserve the existing rejection of single-project analysis overrides
  with groups. Validate values in preflight, before deleting any artifact.
- Use elapsed monotonic time. Keep the deadline active until the direct process
  has exited and output readers have completed. Explicitly close child stdin
  for this noninteractive command boundary.
- On timeout, terminate the owned process tree and reap the direct child; never
  analyze a partial artifact. Report status 1 with the existing
  `coverage_command` diagnostic category, command identity, elapsed limit and
  group where applicable. Do not produce a partial aggregate report.
- Use an owned process group on Unix and evaluate Job Objects for Windows;
  killing the direct child alone cannot satisfy the contract. Cleanup must also
  cover a direct child that has already exited while descendants hold pipes.
  A process-group approach is not containment for deliberately detached children.
- Keep streaming and JSON stdout isolation. Do not add a captured-output limit
  merely to imitate Barney. Reader threads blocked by a parent stderr consumer
  are a separate backpressure problem: process-tree kill alone does not unblock
  a blocked write. Avoid advertising an unconditional wall-clock bound until
  output cancellation and cleanup have been designed and tested.

Acceptance cases: normal success before deadline; hanging runner; parent exits
with live pipe-owning descendant; runner ignores graceful termination; timeout
with partially written artifact; invalid timeout leaves old artifact intact;
noisy dual streams; disabled timeout; group-specific timeout/failure; Linux,
macOS and Windows cleanup. Verify descendants actually exit, not only that the
CLI returns. Timeout classification must win over any late report creation.

Windows Job Objects are a design candidate, not a verified local implementation.
See Microsoft's [Job Objects documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).

## Integration: start with one test run

A documented sequential command is the smallest useful integration:

```json
{
  "scripts": {
    "test:quality": "vitest run --coverage && crap4ts --no-generate"
  }
}
```

Configure Vitest to produce Istanbul JSON at the artifact path in `crap4ts.json`
and include the intended source files. A Jest equivalent uses
`jest --coverage --runInBand && crap4ts --no-generate`, with JSON among its
coverage reporters. These are proposed recipes, not executed runner fixtures.
`&&` preserves a failing test invocation; `--no-generate` avoids a second test
run even if crap4ts has a coverage command configured. The external runner owns
its own timeout here; a crap4ts generation timeout cannot bound an earlier
separate command. An old report still needs cleaning or a runner guarantee of
fresh output; existing-artifact analysis alone does not establish freshness.

If users need a single runner invocation, add a thin adapter that invokes the
existing binary after coverage completes, always with `--no-generate`, and
propagates analysis failure without clearing an existing test failure. Pass argv
without a shell, honor project root and groups, and leave reporting/scoring in
Rust. Do not run tests recursively from the reporter. Watch mode, disabled
coverage, multiple projects and absent artifacts need explicit behavior.

Vitest is the first adapter candidate, but Barney’s coverage-completion hook
is not available to ordinary custom reporters in sampled Vitest 4.0 versions;
Barney locks Vitest 5.0.5. A supported version floor and real integration tests
are needed before adoption. See the companion note for exact sampled versions. Jest needs
separate lifecycle investigation;
a delayed `beforeExit` callback and a five-second file poll should not become
our API contract. Hook availability, compatibility and precise failure behavior
are covered in the companion upstream note.

## Recommendation

Implement the generated-command timeout first, including descendant/pipe cleanup.
Then publish tested one-run Vitest/Jest recipes. Add a Vitest adapter only when
its single-command experience is needed; defer a Jest package until finalization
can be demonstrated against actual supported Jest versions. No new core API,
JUnit format or scoring changes are required by these findings.

## Implementation follow-up, 2026-10-25

Implemented the deadline in [generation.rs](../crates/crap4ts-cli/src/generation.rs)
and exposed `--coverage-timeout-ms` / `coverage.timeout_ms`, including independent
package groups. `command-group` 5.0.1 supplies Unix process groups and Windows Job
Objects. Reader joins are skipped on timeout so inherited pipes cannot hold the
CLI indefinitely. Detached Unix descendants and blocked diagnostic sinks remain
the documented limits. See [usage and recipes](../docs/usage.md#coverage-command-timeout).

Validation: the Rust workspace's 137 tests and clippy pass on macOS; all targets
including tests compile for Windows MSVC. A mutation that killed only the leader
was caught by the descendant-survival regression test. Temporary real-runner
fixtures passed with Vitest/@vitest/coverage-v8 4.1.0 and Jest/babel-jest 30.0.0
on official Node 24.20.0: fresh generation, existing-artifact analysis and a
failing-test command that stops analysis. The fixtures used an existing Babel
TypeScript transform for Jest. No runner reporter packages were introduced.
