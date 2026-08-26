# Existing `crap4ts` implementations

Date: 2026-08-25 (America/Montreal)

## Research question

Which repository returned by GitHub's
[`crap4ts` repository search](https://github.com/search?q=crap4ts&type=repositories)
has the best architecture for a TypeScript CRAP metric quality gate, and which
non-overlapping strengths should inform a new implementation?

The review covered all 11 search results and used
[`unclebob/crap4java`](https://github.com/unclebob/crap4java/tree/69b561209f130ece728f19b0001e90df5a117c3a)
at `69b561209f130ece728f19b0001e90df5a117c3a` as the reference baseline.
Repositories were checked out at fixed commits and inspected through source,
tests, documentation, build commands, and a shared external fixture.

## Reference job

The reference job is a project-quality gate:

1. select source files;
2. acquire or ingest coverage without silently reusing stale evidence;
3. structurally parse concrete function-like units;
4. calculate cyclomatic complexity without including nested function bodies in
   their parents;
5. attribute coverage to the correct function identity;
6. calculate `CRAP = CC² × (1 - coverage)³ + CC`;
7. report numeric results worst-first while preserving unavailable coverage as
   unknown;
8. fail with a distinct status when the configured threshold is exceeded.

Java/Maven mechanics are not invariants. TypeScript/TSX grammar, Istanbul,
LCOV, V8 data, workspaces, package managers, JSON output, and configurable
thresholds are ecosystem-native adaptations.

## Methodology

The methodology was fixed before candidate scoring. Stars, repository size,
implementation language, recency, and visual polish earned no points.

### Viability gates

Each gate was recorded as pass, partial, fail, or not testable:

1. **Reproducible entry:** documented install/build plus help or analysis runs
   from a fresh checkout.
2. **Formula and unknown semantics:** golden vectors pass—`(1, 100%) = 1`,
   `(1, 0%) = 2`, `(2, 50%) = 2.5`—and absent evidence does not become measured
   zero or full coverage.
3. **Structural analysis:** a TypeScript-aware parser handles representative TS
   and TSX without regex-shaped false positives.
4. **Function coverage join:** same-named functions in different files and
   nested functions retain separate identities; unknown differs from zero.
5. **End-to-end quality gate:** one invocation produces identities, complexity,
   coverage, score, deterministic ordering, and meaningful success/failure
   status.
6. **Honest scope:** documented grammar, coverage formats, runners, and outputs
   agree with observed behavior.

A failed gate made a candidate ineligible as the best complete implementation,
even if its implemented parts earned a high architecture score.

### Weighted architecture rubric

Each dimension was rated 0–4, then weighted:

| Dimension | Weight | What was evaluated |
|---|---:|---|
| A. Responsibility boundaries | 25 | Dependency direction; separation of CLI, filesystem/process, parser, coverage, domain, report, and policy |
| B. TypeScript and coverage model | 20 | Grammar coverage, complexity policy, collision-resistant identity, exclusive attribution, explicit unknowns |
| C. Reference and real-world fitness | 15 | Formula, deterministic selection/order, stale coverage, workspaces, CI output and exit behavior |
| D. Test architecture and evidence | 15 | Formula/parser/adapter/application/failure tests plus an executed black-box fixture |
| E. Changeability | 15 | Impact of a second coverage format, a complexity construct, two-package workspace, new report format, and threshold policy |
| F. Operability and kata value | 10 | Install/build/test path, packaging, types, documentation, focused vocabulary, and learnability |

The shared fixture included TypeScript and TSX, nested functions, identical
function names in different files, full/zero/partial/missing Istanbul coverage,
repeat runs, and a threshold breach. Conclusions were triangulated from declared
behavior, static structure, and observed execution.

## Results

Gate columns are `entry / formula / structural / join / E2E / honesty`.
`P` means pass, `p` partial, `F` fail, and `NT` not testable.

| Rank | Candidate | Gates | A | B | C | D | E | F | Total | Complete winner? |
|---:|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| 1 | [`icaruswings`](https://github.com/icaruswings/crap4ts) | P/P/P/P/**F**/P | 4 | 4 | 2 | 4 | 3 | 3 | **86.25** | No: no threshold gate |
| 2 | [`alperlabs`](https://github.com/alperlabs/crap4ts) | P/P/P/P/P/P | 3 | 3 | 3 | 4 | 4 | 3 | **82.50** | **Yes** |
| 3= | [`breezy-bays-labs`](https://github.com/breezy-bays-labs/crap4ts) | P/p/p/P/p/P | 4 | 2 | 2 | 4 | 3 | 4 | **78.75** | Yes, with caveats |
| 3= | [`gligorkot`](https://github.com/gligorkot/crap4ts) | p/p/P/P/P/P | 3 | 4 | 3 | 3 | 2 | 4 | **78.75** | Yes |
| 5 | [`danibram`](https://github.com/danibram/crap4ts) | p/p/P/P/P/P | 3 | 3 | 3 | 3 | 3 | 3 | **75.00** | Yes |
| 6 | [`Jaseempk`](https://github.com/Jaseempk/crap4ts) | P/P/P/P/P/P | 3 | 3 | 3 | 3 | 2 | 3 | **71.25** | Yes |
| 7 | [`fine405`](https://github.com/fine405/crap4ts) (Rust/Oxc) | NT/NT/NT/NT/NT/NT | 2 | 3 | 3 | 2 | 2 | 2 | **58.75** | Insufficient evidence |
| 8 | [`sebassdc`](https://github.com/sebassdc/crap4ts) | P/**F**/P/**F**/P/p | 2 | 2 | 2 | 3 | 2 | 3 | **56.25** | No: missing becomes 100% |
| 9 | [`reaganthomas`](https://github.com/reaganthomas/crap4ts) | P/P/P/p/**F**/P | 2 | 3 | 1 | 2 | 1 | 3 | **50.00** | No: no threshold gate |
| 10 | [`graffhyrum`](https://github.com/graffhyrum/crap4ts) | NT/**F**/NT/p/NT/p | 2 | 3 | 1 | 2 | 2 | 1 | **48.75** | No |
| 11 | [`lukasa1993`](https://github.com/lukasa1993/crap4ts) (Python) | p/P/P/p/P/p | 2 | 2 | 3 | 1 | 1 | 2 | **46.25** | Yes, not competitive |

The raw-score Pareto frontier was Icaruswings, Alperlabs, Breezy, and
Gligorkot. No frontier candidate was no worse than the other three on every
dimension.

## Findings

### Best complete implementation: Alperlabs

Alperlabs was the highest-scoring candidate to pass every viability gate. Its
strongest parts were coverage-reader and report-renderer registries,
module/workspace orchestration, baseline/ratchet policy, change isolation, and
test evidence. Build, lint, typecheck, 187 tests, the shared CLI fixture,
deterministic rows, and threshold failure all passed.

Its important weakness is statement-line range attribution. A statement inside
a nested function can also affect its parent's coverage denominator. Its
unrelated AI-slop feature family dilutes a focused CRAP domain, and the reviewed
dependency tree reported security audit findings.

### Best internal architecture: Icaruswings

Icaruswings had the strongest boundaries, parser/coverage model, diagnostics,
and tests. It preserves unavailable coverage as `null`, uses precise ranges,
excludes nested bodies, separates Istanbul and LCOV, constrains cleanup paths,
and exposes structured errors. All 211 tests and self-analysis passed.

It is deliberately a reporter rather than a quality gate: a completed analysis
returns success regardless of the maximum CRAP score. This localized missing
responsibility disqualified the whole without weakening the architectural value
of its implemented core.

### Best coverage attribution: Gligorkot

Gligorkot assigns Istanbul function identities and statements to exactly one
uniquely most-specific source range. Ties fail closed; an unmatched nested
function does not leak statements into its parent; same-line column collisions
have focused tests. This was the strongest isolated coverage-join design.

Downstream policy still computes a numeric worst-case score when the match flag
is false. A new implementation should keep the ownership algorithm but preserve
that result as unknown. The repository required Node 22 while the review host
had Node 20, so its install/test entry evidence remained partial.

### Best compact baseline-faithful kata: Jaseempk

Jaseempk was the smallest leading implementation to pass all gates. It preserves
unknowns, assigns statements to the innermost method, sorts deterministically,
and implements the reference-style threshold exit. Its application coordinator
is concrete, so new formats and policies require broader edits than in
Alperlabs or Icaruswings.

### Cleanest ports diagram but weaker semantics: Breezy

Breezy had exemplary explicit domain/ports/adapters/core/CLI separation and 583
passing tests. It nevertheless omitted nested function discovery, converted
unmatched coverage to numeric zero, did not sort worst-first by default, and
included timestamps in JSON. It was in maintenance mode and GPL-3.0-or-later,
so it is a design lesson rather than a source for the MIT implementation.

## Language decision: TypeScript first

Version 1 should be TypeScript, not Rust.

1. The target grammar's canonical compiler API is directly available, avoiding
   translation between Rust parser node semantics and TypeScript terminology.
2. The users, fixtures, coverage artifacts, package managers, configuration,
   source maps, and npm distribution are all TypeScript ecosystem concerns.
3. A TypeScript implementation is easier to inspect, modify, and teach as an
   architecture/testing kata.
4. The comparative failures were predominantly semantic—unknown coverage,
   nested attribution, policy, and orchestration—not demonstrated performance
   bottlenecks that Rust would solve.
5. Native binaries or napi bindings introduce platform builds, release
   matrices, and debugging boundaries before there is evidence they are needed.

Rust remains a valid future adapter. Parser and coverage ports must use
library-neutral domain values so an Oxc/napi implementation can replace the
TypeScript parser after profiling, without moving scoring, reporting, threshold
policy, or CLI behavior across the boundary.

## Conclusion for this repository

The implementation should combine non-overlapping strengths rather than fork a
single candidate:

- use Icaruswings-style normalized domain types, precise ranges, explicit
  unknowns, diagnostics, cleanup safety, and test seams;
- use Gligorkot-style exclusive function/statement ownership and ambiguity
  rejection;
- use Alperlabs-style coverage/report adapter registries, workspace
  orchestration, configuration precedence, and ratchet-friendly policy;
- use Jaseempk-style deterministic unknown-last ordering and distinct quality
  gate exit behavior;
- keep the domain focused on CRAP; do not mix unrelated code-quality heuristics
  into the core model;
- implement in TypeScript first and retain a library-neutral parser port for a
  future evidence-driven Rust accelerator.

The highest-value black-box test seam is the packaged CLI: a fixture project and
coverage artifact enter; deterministic stdout, stderr, structured report, and
exit status leave. Formula and adapter contract tests support this seam without
turning module layout into a test contract.
