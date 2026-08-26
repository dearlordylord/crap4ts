# crap4ts

A Rust-based CRAP metric quality gate for TypeScript projects.

The intended pipeline is:

```text
CLI -> source selection -> coverage adapter -> TypeScript analysis
    -> exclusive coverage attribution -> CRAP scoring -> report and gate
```

The implementation is being specified from comparative research rather than
by selecting one existing port as a template. Version 1 consists of a Rust core
and standalone CLI, plus a thin npm wrapper that installs and invokes the
correct prebuilt binary for the user's platform.

## Alternative implementation research

Alternative research is deliberately dated. Later reviews add new entries
instead of silently rewriting the historical basis for an architectural
decision.

- [2026-08-25 — existing `crap4ts` implementations: methodology, comparison,
  and conclusion](./research/2026-08-25-alternative-implementations.md)
- [Research index](./research/README.md)

## Status

The implementation specification is tracked in
[issue #1](https://github.com/dearlordylord/crap4ts/issues/1).

## Development and CLI

The standalone Rust CLI is the first implementation slice. A Rust 1.94 (or
newer) toolchain is required because the Oxc parser is compiled into the core.
From a fresh checkout, run:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The binary consumes an existing Istanbul `coverage-final.json` or LCOV
tracefile artifact and accepts one or more TypeScript source files or
project-relative source roots. Istanbul is the default; select LCOV explicitly
with `--coverage-format lcov`. Sources can be positional or supplied
repeatedly with `--source` (also available as `--source-root`):

```sh
cargo run -p crap4ts -- --help
cargo run -p crap4ts -- --coverage coverage-final.json src --format text
cargo run -p crap4ts -- --coverage coverage-final.json src --format json --threshold 8
cargo run -p crap4ts -- --coverage coverage-final.json --source src --source lib
cargo run -p crap4ts -- --coverage lcov.info --coverage-format lcov src
```

Both formats normalize coverage to the same measured-or-unknown model. For
LCOV, each `DA:<line>,<hits>` record is one denominator unit and a line is a
numerator unit when its hit count is greater than zero. Thus `DA` records with
zero hits are measured zero coverage, while a function with no attributable
`DA` records remains unknown. LCOV has line locations but no columns or
function end ranges: when one line could belong to multiple source functions,
the report keeps those rows unknown and emits a structured coverage-attribution
diagnostic instead of guessing.

LCOV parsing is strict: the supported tracefile records are `TN`, `SF`,
`FN`, `FNDA`, `FNF`, `FNH`, `DA`, `LF`, `LH`, `BRDA`, `BRF`, `BRH`, and
`end_of_record`. Unknown or malformed records fail as coverage-parsing
errors. Function and line summaries are checked against their records. Branch
records are syntax-validated and intentionally ignored for attribution; their
locations are not used to invent columns or function end ranges. Attribution
is based on `DA` lines only.

Exit status `0` means the quality gate passed, `1` means invalid input or
analysis failure, and `2` means a score strictly exceeded the configured
threshold. JSON stdout contains only the versioned report; diagnostics and
quality-gate messages use stderr.

Source discovery accepts only project-local TypeScript identities, supports
`.ts`/`.tsx` (and TypeScript module variants), skips declaration files,
conventional test files, dependency/build/coverage directories, and rejects
symlinked directories. Explicit source paths must exist and be readable;
missing, unreadable, unsupported, or excluded files fail with a
source-selection error, while an empty directory selection fails with a
configuration error. Absolute paths are allowed only when they resolve inside
`--project-root`; parent traversal and symlink escape attempts are rejected.
Each selected directory is also a traversal boundary. Paths are normalized to
`/` only after filesystem resolution, so POSIX and Windows separators produce
the same project-relative identity. Coverage entries must use that identity
(or an absolute path inside `--project-root`); basename and unrelated-path
guesses are rejected.
### npm distribution

The npm distribution is a thin launcher around the same standalone binary:

```sh
npm install --save-dev crap4ts
npx crap4ts --help
```

The meta-package selects an optional native package for Linux x64/arm64,
macOS x64/arm64, or Windows x64. It forwards command-line arguments, standard
streams, signals, and the native process status. It has no postinstall network
download and contains no analysis implementation. A missing optional package
or unsupported platform is reported with an actionable error; unsupported
platforms can use a Cargo-built binary.

The supported Node.js range is `>=20.19.0 <25`, covering the maintained Node
20, 22, and 24 LTS lines. npm and Rust package versions are checked together by
`npm run check:versions`. The committed `package-lock.json` pins the npm
workspace's package relationships.

Maintainers package a prebuilt binary rather than downloading one during npm
installation. For example, a host release artifact can be staged and checked
with:

```sh
cargo build --release -p crap4ts
node scripts/stage-platform.js --target linux-arm64 --binary target/release/crap4ts
npm run pack:npm -- --target linux-arm64 --binary target/release/crap4ts
```

The staging command validates the target's declared `bin` path, and the pack
command fails unless the resulting tarball contains an executable payload. The
clean host archive smoke is run by `npm test`; it builds release mode, packs
the meta and host package, installs them in a temporary consumer, and invokes
help plus fixture analysis. Issue #11 owns the cross-target release matrix and
checksum/publishing automation.

## License

[MIT](./LICENSE)
