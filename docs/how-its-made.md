# How crap4ts is made

crap4ts has a Rust analysis core and standalone CLI. Oxc parses TypeScript;
a thin npm launcher selects and runs the prebuilt executable for the host
platform. The launcher contains no analysis logic and performs no postinstall
network download.

```text
Source selection → coverage adapter → TypeScript analysis
  → exclusive coverage attribution → CRAP scoring → report and gate
```

The core keeps source discovery, Istanbul/LCOV adapters, function analysis,
coverage attribution, scoring, and reporting separate. Nested function bodies
are analyzed separately; unavailable or ambiguous coverage remains unknown.
The CLI handles configuration, coverage commands, and independent package groups.

The design draws on [comparative implementation research](../research/2026-08-25-alternative-implementations.md).
A [Rust architecture reassessment](../research/2026-08-25-rust-architecture-reassessment.md)
superseded the initial TypeScript-first plan. The accepted
[architecture decision](./adr/0001-rust-core-and-thin-npm-wrapper.md)
records the trade-off: a runtime-independent executable with a five-target
native release matrix. Further investigations are in the [research index](../research/README.md).

## Development

The standalone Rust CLI is the primary interface. A Rust 1.94 (or
newer) toolchain is required because the Oxc parser is compiled into the core.
From a fresh checkout, run:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
npm ci --force
npm test
```

`--force` is limited to the workspace install because npm otherwise rejects
the checked-in non-host native workspaces before applying their optional
dependency filters. Published consumers do not need this flag.

## Packaging

Maintainers package a prebuilt binary rather than downloading one during npm
installation. For example, on a Linux arm64 host, stage and check a release
artifact with:

```sh
cargo build --release -p crap4ts
node scripts/stage-platform.js --target linux-arm64 --binary target/release/crap4ts
npm run pack:npm -- --target linux-arm64 --binary target/release/crap4ts
```

The staging command validates the target's declared `bin` path, and the pack
command fails unless the resulting tarball contains an executable payload. The
clean host archive smoke is run by `npm test`; it builds release mode, packs
the meta and host package, installs them in a temporary consumer, and invokes
help plus fixture analysis.

## Release

Maintainers can assemble and verify without publishing anything:

```sh
npm run check:versions
npm run check:schemas
node scripts/release.js assemble --binary-dir dist/binaries --output-dir dist/release
node scripts/npm-smoke.js --release-dir dist/release --binary dist/binaries/crap4ts-linux-x64 --marker dist/release/.smoke.ok
node scripts/release-gate.js --release-dir dist/release \
  --binary-manifest /path/to/trusted/BINARY-SHA256SUMS
```

The binary manifest is a separate immutable build artifact downloaded by the
release workflow; the gate compares it with the in-release copy and every
standalone/npm binary payload.

The gate refuses a release with any missing target archive, checksum, npm
package, required binary payload, or smoke marker. A maintainer releases from a
clean, pushed, green `master` checkout after authenticating `gh` and `npm`:

```sh
pnpm local-release
```

That command creates the version tag, waits for GitHub Actions to cross-build
all five targets, downloads and re-verifies the immutable artifacts, publishes
the five native packages before `@crap4ts/crap4ts`, and finalizes the GitHub
release last. It is safe to retry: identical remote bytes are skipped and
conflicting bytes stop the release. `pnpm local-release --check` performs only
the read-only preflight.

[Usage reference](./usage.md) · [Back to README](../README.md)
