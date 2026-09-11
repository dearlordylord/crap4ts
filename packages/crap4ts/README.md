# crap4ts

Inspired by Robert C. Martin (Uncle Bob)'s
[crap4java](https://github.com/unclebob/crap4java) and
[crap4clj](https://github.com/unclebob/crap4clj), adapted for TypeScript and TSX.

Find complex, poorly tested TypeScript functions and fail CI when their risk
exceeds your limit. crap4ts combines cyclomatic complexity and test coverage
into a per-function CRAP (Change Risk Anti-Patterns) score.

It analyzes TypeScript/TSX with Istanbul JSON or LCOV coverage and produces
text or JSON reports. Supports per-file thresholds and independent monorepo
package groups.

## Quick start

Requires Node.js `>=20.19.0 <25`. Native packages support Linux (glibc 2.35+) and macOS
on x64/arm64, plus Windows x64.

```sh
npm install --save-dev @crap4ts/crap4ts
# Generate coverage with your test runner first, then:
npx crap4ts --coverage coverage/coverage-final.json src
```

For LCOV or JSON output:

```sh
npx crap4ts --coverage coverage/lcov.info --coverage-format lcov src
npx crap4ts --coverage coverage/coverage-final.json src --format json --threshold 12
```

## Using crap4ts in an agent workflow

Give a coding agent concrete feedback on complex, poorly tested functions:
generate coverage, run crap4ts, inspect flagged functions, improve tests or
simplify the code, then rerun the gate. JSON reports support automated feedback;
the same thresholds can run in CI.

See the [agent workflow recipe](https://github.com/dearlordylord/crap4ts/blob/master/docs/usage.md#agent-workflow-recipe)
for configuration and reusable agent instructions.

## Scoring and CI

`CRAP = complexity² × (1 − coverage)³ + complexity`, where coverage is a
fraction from 0 to 1. Lower scores are better.

The default threshold is **8**. Exit codes: **0** passes, **2** means a score
exceeds its threshold, **1** means invalid input or analysis failure.
Missing or ambiguous coverage fails by default; `--report-only` keeps unknown
scores visible while still enforcing thresholds on measured scores.

Tests, declarations, and generated output are excluded by default. JavaScript
sources and raw V8 coverage are unsupported.

[Usage and configuration](https://github.com/dearlordylord/crap4ts/blob/master/docs/usage.md)
· [How it’s made](https://github.com/dearlordylord/crap4ts/blob/master/docs/how-its-made.md)
· [MIT license](https://github.com/dearlordylord/crap4ts/blob/master/LICENSE)
