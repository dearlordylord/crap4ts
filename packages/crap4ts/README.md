# crap4ts

`crap4ts` is the npm distribution of the standalone Rust CRAP quality-gate
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
npm install --save-dev crap4ts
npx crap4ts --help
```

There is no postinstall download. If npm optional dependencies are disabled,
or a platform package is missing, the executable reports the package and
platform that need to be installed. Unsupported platforms can use the direct
Cargo-built binary instead.
