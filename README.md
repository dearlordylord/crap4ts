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

Exit status `0` means the quality gate passed, `1` means invalid input or
analysis failure, and `2` means a score strictly exceeded the configured
threshold. JSON stdout contains only the versioned report; diagnostics and
quality-gate messages use stderr.

Source discovery accepts only project-local TypeScript identities, supports
`.ts`/`.tsx` (and TypeScript module variants), skips declaration files,
conventional test files, dependency/build/coverage directories, and rejects
symlinked directories. Explicit source paths must exist and be readable;
missing or unreadable paths fail with a source-selection error, while an empty
selection fails with a configuration error. Absolute paths are allowed only
when they resolve inside `--project-root`; parent traversal and symlink escape
attempts are rejected. Paths are normalized to `/`, so POSIX and Windows
separators produce the same project-relative identity. Coverage entries must
use that identity (or an absolute path inside `--project-root`); basename and
unrelated-path guesses are rejected.

## License

[MIT](./LICENSE)
