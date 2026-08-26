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

The binary consumes an existing Istanbul `coverage-final.json` artifact and
accepts one or more TypeScript source files or project-relative source roots.
Sources can be positional or supplied repeatedly with `--source` (also
available as `--source-root`):

```sh
cargo run -p crap4ts -- --help
cargo run -p crap4ts -- --coverage coverage-final.json src --format text
cargo run -p crap4ts -- --coverage coverage-final.json src --format json --threshold 8
cargo run -p crap4ts -- --coverage coverage-final.json --source src --source lib
```

The same inputs can be supplied by a project-local `crap4ts.json` file (the
`.crap4ts.json` and `crap4ts.config.json` spellings are also discovered). The
configuration is JSON data only; executable JavaScript configuration and
unknown fields are rejected. A minimal configuration is:

```json
{
  "sources": ["src"],
  "coverage": "coverage-final.json",
  "format": "json",
  "threshold": 8,
  "thresholds": {"src/legacy.ts": 12},
  "missing_evidence": "error"
}
```

`coverage` may also be an object with `path` and `format` fields. `report` or
`reports` may contain `format: "text"` or `format: "json"`. Set
`missing_evidence` to `"report_only"` (or use `report_only: true`) to retain
unknown rows without assigning them a score. Threshold keys are exact
project-relative paths; separators are normalized and basename or glob
matching is never used.

Values are resolved in this order: built-in defaults, project configuration,
then explicitly supplied CLI options. The default global threshold is `8`,
and a gate fails only when a measured score is strictly greater than its
effective global or path threshold. `--config PATH` selects a configuration
explicitly; its file must remain inside `--project-root`.

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
