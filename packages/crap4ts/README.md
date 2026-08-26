# crap4ts

`@crap4ts/crap4ts` is the npm distribution of the standalone Rust CRAP quality-gate
CLI. The package contains only a launcher. At install time npm selects one
matching optional platform package; the launcher passes arguments, standard
streams, signals, and the child exit status through to that native binary.

Supported package targets are:

- Linux x64 (`@crap4ts/linux-x64`)
- Linux arm64 (`@crap4ts/linux-arm64`)
- macOS x64 (`@crap4ts/darwin-x64`)
- macOS arm64 (`@crap4ts/darwin-arm64`)
- Windows x64 (`@crap4ts/win32-x64`)

The supported Node.js range is `>=20.19.0 <25`, covering the maintained Node
20, 22, and 24 LTS lines. Install with:

```sh
npm install --save-dev @crap4ts/crap4ts
npx crap4ts --help
```

There is no postinstall download. If npm optional dependencies are disabled,
or a platform package is missing, the executable reports the package and
platform that need to be installed. Unsupported platforms can use the direct
Cargo-built binary instead.

The CLI reads `crap4ts.json` (or `.crap4ts.json`/`crap4ts.config.json`) as
strict JSON. A project can use an existing Istanbul artifact (the default), an
LCOV tracefile (`--coverage-format lcov`), or a direct argv coverage command.
Conventional tests and generated directories are excluded from discovery by
default; `source_filters.include_tests` and
`source_filters.include_generated` opt them in, while `.git` and
`node_modules` remain permanently excluded.
Generated commands are never shell-split; Windows `.cmd`/`.bat` shims are
rejected, so use a native `.exe` (for example `node.exe` plus npm-cli.js) rather than
`npm` in a command array. Generated coverage is deleted and recreated only
inside the project root, and child output is forwarded to stderr.

Use `--format text` for a terminal report or `--format json` for the stable
versioned contract. Single-project output is [JSON schema v1](https://github.com/dearlordylord/crap4ts/blob/master/schemas/report-v1.schema.json);
independent package groups use [schema v2](https://github.com/dearlordylord/crap4ts/blob/master/schemas/report-v2.schema.json).
Exit status `0` is a passing gate, `1` is invalid input/analysis failure, and
`2` is a measured score above its threshold. Unknown coverage remains unknown;
`--report-only` only keeps those rows in the report.

The native target set is Linux x64/arm64 (glibc), macOS x64/arm64, and Windows
x64. The npm wrapper contains no analyzer implementation and does not download
executables at install time. JavaScript sources, raw V8 coverage, source-map
reconstruction, HTML/SARIF reports, baselines, and changed-lines gates are not
part of v1. Standalone release archives, `SHA256SUMS`, and the separate
`npm/NPM-SHA256SUMS` manifest are
available for users who do not use npm.

For operators installing from a release archive, choose the archive whose name
ends in `linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`, or
`win32-x64`; verify its SHA-256 entry before extraction. The archive includes
the executable, license, and operator documentation.
