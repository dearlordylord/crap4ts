# Rust architecture reassessment

This entry supersedes only the implementation-language conclusion in the
earlier [alternative implementation research](./2026-08-25-alternative-implementations.md).
That original assessment remains unchanged as historical evidence.

Version 1 uses a Rust core and standalone CLI with an Oxc TypeScript source
adapter. The product's primary contract is an independent quality-gate
executable, not an embedded JavaScript library. Rust avoids coupling execution
to a consumer's Node version, module system, or package manager, while Oxc
provides the structural TypeScript/TSX parsing needed by the analysis boundary.

The npm distribution remains a thin launcher. It selects one prebuilt native
package and contains no analysis implementation. This choice accepts the cost
of a five-target build and release matrix in exchange for runtime-independent
binaries and direct checksummed downloads.

The language-neutral domain boundaries identified by the comparison remain in
force: parser objects, coverage formats, process execution, scoring, policy,
and rendering stay separated. The architecture decision is also recorded in
[ADR 0001](../docs/adr/0001-rust-core-and-thin-npm-wrapper.md).
