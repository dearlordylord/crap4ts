---
status: accepted
---

# Rust core and thin npm wrapper

Use a Rust core and standalone CLI with Oxc behind the TypeScript analysis
boundary, then distribute the same executable directly and through a thin npm
platform selector. This supersedes the research's initial TypeScript-first
choice because the primary product is a runtime-independent quality gate; the
trade-off is maintaining a five-target native release matrix.
