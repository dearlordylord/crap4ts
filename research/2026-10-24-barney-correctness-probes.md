# Barney: executable correctness and integration follow-up

Review date: 2026-10-24. Follow-up to the [competitor refresh](./2026-10-24-competitor-refresh.md).
Upstream source: [`e201242fec7bc06ba5c8c65bcc98f853681fd31c`](https://github.com/fabian-barney/crap-typescript/tree/e201242fec7bc06ba5c8c65bcc98f853681fd31c).
Local source: `dfef61b`. Probes use a build of pinned source, not the published npm tarballs.

## Executed results

| Probe | Barney | Local crap4ts |
|---|---|---|
| Anonymous `map` callback containing an `if` | Only `outer`, CC 1; callback absent | `outer`, CC 1, and anonymous arrow, CC 2 |
| Named nested function containing an `if` | Separate `outer` CC 1 and `inner` CC 2 | Not rerun here |
| Selected source, existing Istanbul report `{}` | API max CRAP 0; `runCli` returns 0 even at threshold 1 | Strict CLI returns 1; report-only keeps unknown rows |
| Covered statements, expected but absent branches | Effective coverage 100%; branch metric unknown | Not compared; local branch mode explicitly supports fallback too |

### Anonymous callback decisions disappear

```ts
export function outer(xs: number[]) {
  return xs.map(x => { if (x) return 1; return 0; });
}
```

`parseFileMethods` returned only `outer`, complexity 1. The callback has no
assignment name recognized by the descriptor builders, while the outer
complexity walk stops at nested function boundaries. The `if` therefore belongs
to no reported unit. Naming a nested declaration produces its own row.
This is independent of coverage availability.
Source: [parser.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/parser.ts),
especially `descriptorFromAssignedFunction`, `findAssignedFunctionName`, and `countCyclomaticComplexity`.

The local CLI, rebuilt with `cargo run -q -p crap4ts`, returned two rows with
complexities 1 and 2 using `--report-only --format json` on the same source.
Strict mode on the same empty coverage report returned status 1.
Local source: [source.rs](../crates/crap4ts-core/src/source.rs).

### Unknown evidence passes the threshold gate

For a selected conditional function and `coverage/coverage-final.json` containing
`{}`, `analyzeProject({coverageMode: "existing-only", threshold: 1, ...})` returned:

```json
{"maxCrap":0,"thresholdExceeded":false,"coverage":{"percent":null,"status":"unknown","unknownReason":"file_unmatched"}}
```

`runCli(["--format", "json", "--threshold", "1", file], root, ...)` also returned
**0**. The existing empty report prevented test execution. Unknown survives in
the data but does not fail the gate. Both adapter reporters check the same
`thresholdExceeded` result; unknowns through those hooks were not separately probed.
Sources: [analyzeProject.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/analyzeProject.ts),
[crapScore.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/crapScore.ts),
[cli.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/cli.ts).

### Partial coverage uses the measured component

Given a parsed conditional descriptor, `coverageForMethods` received one covered
synthetic statement spanning its body, no function-map entries and no branches.
Result: statement coverage 100%, branch unknown (`branch_unattributed`), combined
coverage measured at 100%. This exercises normalized attribution, not actual
instrumenter output. It qualifies the headline minimum-of-two scoring claim;
it is not by itself a local correctness advantage because our documented branch
mode also permits evidence fallback.
Source: [coverageNormalization.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/coverageNormalization.ts).

## Integration lessons from source

- **Vitest:** adds JSON coverage and analyzes in `onFinishedReportCoverage` with
  `existing-only`, avoiding a second test run. Explicitly disabled coverage
  prevents its reporter attachment. This is a useful small integration seam.
  [Source](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/vitest/src/index.ts).
- **Jest:** schedules finalization on `beforeExit`, waits up to 5 seconds for
  coverage, sets `process.exitCode` on failure. This requires more lifecycle
  coordination than Vitest's hook.
  [Source](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/jest/src/reporter.ts).
- **Existing coverage first:** automatic mode reuses a report before running
  tests; `ensureCoverageReport` has no freshness check. Reuse does not establish
  correspondence with the current checkout. Our explicit generation is useful.
  [Source](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/coverage.ts).
- **Discovery:** `.ts`/`.tsx` only, with nested `src` trees for directory discovery.
  Our `.mts`/`.cts` and configured roots remain differences.
  [Source](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/fileSelection.ts).

## Validation and limits

Isolated temporary clone; `npm ci --ignore-scripts --no-audit --no-fund` and
`npm run build` succeeded. Install/build used Node 25.6.1, outside upstream's
development engine (npm warned). Probes and tests used supported **Node 24.20.0**.
Core tests: **199/199**, 14 files. Vitest/Jest adapter tests: **47/47**, 6 files,
including their integration tests. Commands (with Node 24 first on PATH):

```sh
node node_modules/vitest/vitest.mjs run packages/core/test --coverage.enabled=false
node node_modules/vitest/vitest.mjs run packages/vitest/test packages/jest/test --coverage.enabled=false
```

Assertions in the temporary API probe checked null CRAP, non-failing unknown
gate, measured 100% alongside unknown branches, absent anonymous callback,
and CLI exit 0. Passing upstream tests do not invalidate those observations.
No complete release/lint/self-CRAP battery, performance benchmark, published
tarball equivalence check, or full Unicode/same-line matrix was performed.

Keep Barney in the active roster. Study bounded subprocess execution and
coverage-completion hooks; retain independent anonymous function units and
strict missing-evidence enforcement. No application code changed.
