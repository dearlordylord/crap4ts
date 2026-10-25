# Competitor refresh and Uncle Bob

Review date: 2026-10-24. Local revision: `dfef61b` (clean at review start).
This supplements the [August architecture review](./2026-08-25-alternative-implementations.md)
and supersedes dated release/status conclusions where noted below.

## What changed

The most useful new competitor to study is **fabian-barney/crap-typescript**:
a published toolkit with CLI, core API, Vitest and Jest packages. **Icaruswings
now has a threshold gate and public npm publication**, removing the specific
reason it was disqualified in August. **Graffhyrum has continued to v2.1.3**,
although its attribution and unknown-score semantics still need caution.
**Bob's TypeScript implementation has not changed since our October 3 review**.

These are follow-up priorities, not a rerun of the August weighted ranking.
No new comparative benchmark or complete upstream test-suite run was performed.
The subsequent [Barney follow-up](./2026-10-24-barney-correctness-probes.md)
adds executable probes and a partial upstream test run; limits below describe
this initial source/release refresh.

## Source and release inventory

Each tree link pins the default-branch revision resolved through GitHub's repository
and commit APIs. Dates are **HEAD commit dates**, not repository `pushed_at`;
tag pushes can make the latter look newer. “None returned” means the Releases
endpoint returned no release objects; it does not rule out tags or registry packages.
The release column is the first object returned by `releases?per_page=1`, and can
be an announcement or prerelease rather than the highest stable version.

| Repository | Default HEAD | HEAD date (UTC) | Latest returned GitHub release |
|---|---|---|---|
| [icaruswings/crap4ts](https://github.com/icaruswings/crap4ts/tree/bf5b556d55c6d6bb49edeee5e080e9479f0b2801) | `bf5b556d55c6` | 2026-09-23 | None returned |
| [alperlabs/crap4ts](https://github.com/alperlabs/crap4ts/tree/b2026f9f4c3ec29851a381cc852bf1e5d438b0a6) | `b2026f9f4c3e` | 2026-07-30 | [v0.2.1](https://github.com/alperlabs/crap4ts/releases/tag/v0.2.1) |
| [breezy-bays-labs/crap4ts](https://github.com/breezy-bays-labs/crap4ts/tree/6d71f9cabd490838c935146d2334f869931d079e) | `6d71f9cabd49` | 2026-05-10 | [v1.x-maintenance-announcement](https://github.com/breezy-bays-labs/crap4ts/releases/tag/v1.x-maintenance-announcement) |
| [gligorkot/crap4ts](https://github.com/gligorkot/crap4ts/tree/e1259b0ae20205596c9a3557c46e48717fa44890) | `e1259b0ae202` | 2026-08-26 | [v1.1.0](https://github.com/gligorkot/crap4ts/releases/tag/v1.1.0) |
| [danibram/crap4ts](https://github.com/danibram/crap4ts/tree/41e37bb986370e88861f42ce1a7eff6ecb5826d8) | `41e37bb98637` | 2026-05-28 | [v0.6.0](https://github.com/danibram/crap4ts/releases/tag/v0.6.0) |
| [Jaseempk/crap4ts](https://github.com/Jaseempk/crap4ts/tree/72ce9cc05a441d0cd5a104534849601f9782dbe6) | `72ce9cc05a44` | 2026-08-06 | None returned |
| [fine405/crap4ts](https://github.com/fine405/crap4ts/tree/9ab5dffed343a63ee1c239f92c4846c8e49ccb97) | `9ab5dffed343` | 2026-08-25 | None returned |
| [sebassdc/crap4ts](https://github.com/sebassdc/crap4ts/tree/340d0e2c62fad70dcf737b8a479cf1b058d1d9fc) | `340d0e2c62fa` | 2026-04-14 | None returned |
| [reaganthomas/crap4ts](https://github.com/reaganthomas/crap4ts/tree/1565762c7300e75b353898305ca6f923ea42e72a) | `1565762c7300` | 2026-05-09 | None returned |
| [graffhyrum/crap4ts](https://github.com/graffhyrum/crap4ts/tree/4e70edf1dfb20dc78591a2ef0eead2d298ba9bc8) | `4e70edf1dfb2` | 2026-10-22 | [v2.1.3](https://github.com/graffhyrum/crap4ts/releases/tag/v2.1.3) |
| [lukasa1993/crap4ts](https://github.com/lukasa1993/crap4ts/tree/e7ff9f21978ca3de292f335f53fdb9875d4f3a33) | `e7ff9f21978c` | 2026-08-22 | None returned |
| [breezy-bays-labs/crap-rs](https://github.com/breezy-bays-labs/crap-rs/tree/20609955815975d731256db57a819c839b5ab77c) | `206099558159` | 2026-06-25 | [crap4ts-v2.0.0-rc.5](https://github.com/breezy-bays-labs/crap-rs/releases/tag/crap4ts-v2.0.0-rc.5) |
| [unclebob/crapper](https://github.com/unclebob/crapper/tree/6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d) | `6fd92c4c9bcc` | 2026-10-21 | None returned |
| [fabian-barney/crap-typescript](https://github.com/fabian-barney/crap-typescript/tree/e201242fec7bc06ba5c8c65bcc98f853681fd31c) | `e201242fec7b` | 2026-10-24 | [v2.0.0](https://github.com/fabian-barney/crap-typescript/releases/tag/v2.0.0) |
| [ardsh/crap4ts](https://github.com/ardsh/crap4ts/tree/2a5f7e38af614d776fc8c1e97c70eaa105112ff9) | `2a5f7e38af61` | 2026-05-27 | [v1.0.3](https://github.com/ardsh/crap4ts/releases/tag/v1.0.3) |

The original eleven candidates are retained; Breezy's Rust successor, Bob's
multi-language tool, Barney, and Ardsh extend the roster. Among these fifteen
default branches, only Barney, Graffhyrum, and Bob have HEAD commit dates in
October. This is activity evidence, not correctness evidence.

### Registry evidence

The npm registry's `dist-tags`, version timestamps, and version-level repository
metadata were checked directly. These are publications, not manifest versions.

| Package | Latest stable tag | Attribution |
|---|---|---|
| [@icaruswings138/crap4ts](https://registry.npmjs.org/@icaruswings138%2Fcrap4ts) | 0.4.0, published Sep 23 | icaruswings/crap4ts |
| [@alperlabs/crap4ts](https://registry.npmjs.org/@alperlabs%2Fcrap4ts) | 0.2.1 | alperlabs/crap4ts |
| [crap4ts](https://registry.npmjs.org/crap4ts) | 1.0.1 | breezy-bays-labs/crap4ts |
| [@gligor/crap4ts](https://registry.npmjs.org/@gligor%2Fcrap4ts) | 1.1.0 | gligorkot/crap4ts |
| [@danibram/crap4ts](https://registry.npmjs.org/@danibram%2Fcrap4ts) | 0.6.0 | danibram/crap4ts |
| [@sebassdc/crap4ts](https://registry.npmjs.org/@sebassdc%2Fcrap4ts) | 0.1.1 | sebassdc/crap4ts |
| [@sql-engine/crap4ts](https://registry.npmjs.org/@sql-engine%2Fcrap4ts) | 1.0.3 | ardsh/crap4ts |
| [@barney-media/crap-typescript](https://registry.npmjs.org/@barney-media%2Fcrap-typescript) | 2.0.0, published Oct 18 | fabian-barney/crap-typescript |
| [@barney-media/crap-typescript-core](https://registry.npmjs.org/@barney-media%2Fcrap-typescript-core) | 2.0.0, published Oct 18 | fabian-barney/crap-typescript |
| [@barney-media/crap-typescript-vitest](https://registry.npmjs.org/@barney-media%2Fcrap-typescript-vitest) | 2.0.0, published Oct 18 | fabian-barney/crap-typescript |
| [@barney-media/crap-typescript-jest](https://registry.npmjs.org/@barney-media%2Fcrap-typescript-jest) | 2.0.0, published Oct 18 | fabian-barney/crap-typescript |
| [@crap4ts/crap4ts](https://registry.npmjs.org/@crap4ts%2Fcrap4ts) (ours) | 1.1.0, published Oct 4 | dearlordylord/crap4ts |

Unscoped `crap4ts` also has an `rc` tag at `2.0.0-rc.5`; its successor lives
in Breezy's `crap-rs`, so do not conflate the two repositories' releases.
Public npm returned 404 for `@graffhyrum/crap4ts`, but its
[manifest](https://github.com/graffhyrum/crap4ts/blob/4e70edf1dfb20dc78591a2ef0eead2d298ba9bc8/package.json)
targets **GitHub Packages**, with restricted access. Its GitHub release is verified;
the installed GitHub Packages artifact was not independently fetched. Registry
publication was not exhaustively rechecked for every remaining roster row.

## Barney: the most relevant new follow-up

Source inspected at `e201242fec7bc06ba5c8c65bcc98f853681fd31c`.

- **Integration advantage:** separate published core, CLI, Vitest helper and Jest
  reporter reduce setup friction. JUnit sidecars and compact TOON/agent reporting
  serve CI and agent workflows beyond our text/JSON CLI.
  Sources: [packages](https://github.com/fabian-barney/crap-typescript/tree/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages),
  [reporter](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/report.ts).
- **Substantive coverage model:** statements and branches are assigned to one
  selected owning method; ambiguous function-map matches become unavailable.
  This deserves a shared fixture comparison rather than dismissing it as a
  convenience wrapper.
  Source: [coverageAttribution.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/coverageAttribution.ts).
- **Scoring differs:** when both components are measured, effective coverage
  is their minimum. If only one is measured, it uses that component even when
  the other is unknown. Proven structural non-applicability is separate from
  missing evidence. Thus the headline “minimum of statement and branch” needs
  qualification for incomplete evidence.
  Source: [coverageNormalization.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/coverageNormalization.ts).
- **Unknowns are preserved but do not themselves fail the threshold gate:**
  null CRAP becomes a skipped method, `maxCrap` ignores null, and CLI threshold
  status depends on that numeric maximum. Our strict default remains a meaningful
  policy distinction.
  Sources: [crapScore.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/crapScore.ts),
  [report.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/report.ts),
  [cli.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/cli.ts).
- **Process controls worth borrowing:** built-in commands default to 300 seconds,
  configurable through `CRAP_TYPESCRIPT_COMMAND_TIMEOUT_MS` (0 disables it);
  captured output is bounded to 10 MiB per stream.
  Source: [utils.ts](https://github.com/fabian-barney/crap-typescript/blob/e201242fec7bc06ba5c8c65bcc98f853681fd31c/packages/core/src/utils.ts).

No claim of full equivalence or superiority is warranted without executing
same-line, nested-function, Unicode-column and partial-evidence fixtures through
both packaged CLIs.

## Icaruswings: the old disqualification is obsolete

The source now exposes `--threshold` and returns status **3** when a measured
score exceeds it. Without a threshold it remains a reporter; null scores are
excluded from threshold checks. Combined with verified npm 0.4.0 publication,
this invalidates the August statements “no threshold gate” and “not released.”
It does not retroactively change the August measurements.
Sources: [check-threshold.ts](https://github.com/icaruswings/crap4ts/blob/bf5b556d55c6d6bb49edeee5e080e9479f0b2801/src/cli/check-threshold.ts),
[main.ts](https://github.com/icaruswings/crap4ts/blob/bf5b556d55c6d6bb49edeee5e080e9479f0b2801/src/cli/main.ts),
[npm record](https://registry.npmjs.org/@icaruswings138%2Fcrap4ts).

## Graffhyrum: active, but preserve the caveats

v2.1.3 offers Bun execution, Istanbul/LCOV/V8 ingestion and HTML output.
Current matching preserves a missing file as null coverage, but the scoring
function converts null to zero before producing a numerical worst-case score.
Within a covered file, statement selection and subtraction of nested functions
use line ranges, so column-level ambiguity remains unresolved by this algorithm.
These are static source observations, not newly executed failure cases.
Sources: [pipeline.ts](https://github.com/graffhyrum/crap4ts/blob/4e70edf1dfb20dc78591a2ef0eead2d298ba9bc8/src/pipeline.ts),
[match.ts](https://github.com/graffhyrum/crap4ts/blob/4e70edf1dfb20dc78591a2ef0eead2d298ba9bc8/src/mapping/match.ts),
[score.ts](https://github.com/graffhyrum/crap4ts/blob/4e70edf1dfb20dc78591a2ef0eead2d298ba9bc8/src/crap/score.ts).
The useful lessons are report presentation and runner setup; no evidence here
supports replacing our coverage join.

## Bob: one Python fix, no new TypeScript behavior

Compared with the October 3 pinned revision `9f1bead298b5a9d576bdd6319289fcf426e5b18a`,
current HEAD has exactly one additional commit: top-level Python source files
are converted to module names for coverage.py, with a regression test.
The diff changes only `runners.py` and `tests/test_runners.py`. Consequently
the prior TypeScript extraction/scoring findings remain applicable; this refresh
does not establish a new TypeScript advantage.
Source: [pinned comparison](https://github.com/unclebob/crapper/compare/9f1bead298b5a9d576bdd6319289fcf426e5b18a...6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d).

Our previous follow-up list is already substantially implemented: optional-chain
decisions, opt-in branch scoring, route labels, working-tree change selection
and explicit Git-base selection. See [source.rs](../crates/crap4ts-core/src/source.rs),
[usage](../docs/usage.md), and [README](../README.md).
Do not keep those as open competitive gaps based on the October 3 snapshot.

## Recommended next work

1. Compare Barney and our packaged CLI on a small shared correctness fixture.
   Its integration and reporting surfaces make it a more useful current target
   than repeatedly inspecting unchanged Sebassdc source.
2. Prioritize coverage-command timeout, then a minimal Vitest/Jest integration
   or a documented equivalent if setup friction is the problem. Borrow JUnit
   output only when a concrete CI consumer needs it.
3. Retain explicit unknown evidence, strict default enforcement, independent
   nested function units and precise coverage attribution. Avoid claiming that
   competing tools have no structured unknown model.
4. Keep Bob as a multi-language reference; there is no new TypeScript work to
   port from his latest commit.

## Limits

This is a dated source/release refresh of the known roster plus two discovered
TypeScript repositories, not an exhaustive market census or a repeated weighted
architecture evaluation. Source clones were read outside the workspace; no
upstream dependencies were installed and no upstream suite or performance
benchmark was run. Prior bug probes were not rerun for unchanged Sebassdc HEAD.
Repository source, registry publication and advertised features are distinguished
throughout; distribution success on every supported platform was not retested.

