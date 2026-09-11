# Usage reference

[Back to README](../README.md)

The binary consumes an existing Istanbul `coverage-final.json` or LCOV
tracefile artifact and accepts one or more TypeScript source files or
project-relative source roots. Istanbul is the default; select LCOV explicitly
with `--coverage-format lcov`. Sources can be positional or supplied
repeatedly with `--source` (also available as `--source-root`):

```sh
npx crap4ts --help
npx crap4ts --coverage coverage-final.json src --format text
npx crap4ts --coverage coverage-final.json src --format json --threshold 8
npx crap4ts --coverage coverage-final.json --source src --source lib
npx crap4ts --coverage lcov.info --coverage-format lcov src
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

Discovery excludes conventional test files/directories and generated output by
default. A project that intentionally analyzes those sources can opt in with
`"source_filters": {"include_tests": true, "include_generated": true}` at the
top level or within an individual package group. `.git` and `node_modules`
remain excluded and cannot be enabled.

To generate fresh coverage in the same invocation, add an argv command to the
coverage object (or use the equivalent top-level `coverage_command` field):

```json
{
  "sources": ["src"],
  "coverage": {
    "path": "coverage-final.json",
    "format": "istanbul",
    "command": ["npm", "test", "--", "--coverage"]
  },
  "format": "json"
}
```

The first array element is executed as the program and the remaining elements
are passed as-is; crap4ts does not split a string or invoke a shell. A shell
can be requested explicitly as a program (for example, `["sh", "-c", ...]`).
Before generated mode starts, only the explicitly configured artifact can be
removed. The path must remain below the project root, contain no parent
traversal, and have no symlink file or ancestor. The command must exit
successfully and create a readable regular artifact; an older artifact is
never reused after a failed or incomplete command. Child stdout and stderr are
forwarded to stderr, so JSON stdout remains a single report document. Use
`--coverage-command PROGRAM` and repeated `--coverage-arg ARG` for a command
provided directly on the CLI. `--no-generate` selects the existing-artifact
path even when a command is configured.

## Independent package groups

Monorepos can declare independent package analyses with a `groups` object (or
an array of objects containing a `name` field):

```json
{
  "format": "json",
  "groups": {
    "core": {
      "root": "packages/core",
      "sources": ["src"],
      "coverage": {
        "path": "coverage-final.json",
        "format": "istanbul",
        "command": ["npm", "test", "--", "--coverage"]
      },
      "threshold": 8
    },
    "web": {
      "root": "packages/web",
      "sources": ["src"],
      "coverage": {
        "path": "coverage/lcov.info",
        "format": "lcov"
      },
      "threshold": 12,
      "missing_evidence": "report_only"
    }
  }
}
```

Each group root, source path, coverage artifact, and command is resolved below
the repository root. Sources and artifacts are relative to the group's root,
and a command runs with that root as its working directory. Group names must
be unique; selected source files and generated artifacts cannot be shared.
All groups are preflighted before generated artifacts are removed or commands
run. Analysis is acquired and completed in memory for every group before one
aggregate report is rendered, so a failed group never produces a partial
report.

Aggregate JSON is schema version 2. Rows contain a structural `group` field,
repository-root-relative `path` values, and group-qualified `id` values.
Diagnostics carry the originating `group`, while `groups` records each
package root, threshold, missing-evidence mode, and exact path overrides. Rows are sorted globally by
CRAP score (worst first), then group, path, and source position. Group
declarations are sorted by name for canonical output.
Version 2 omits the legacy top-level `threshold` scalar because no single
threshold applies to all groups; consumers must read each group's policy from
`groups`. Version 1 single-project reports retain that scalar and their
existing JSON shape.

The existing single-project analysis flags (`--coverage`, source paths,
`--coverage-format`, generation flags, `--threshold`, and missing-evidence
flags) are intentionally rejected when `groups` is configured; they cannot
be broadcast implicitly. `--format`, `--json`, `--config`, and
`--project-root` remain invocation-global controls. Configure every analysis
setting on its named group.

Values are resolved in this order: built-in defaults, project configuration,
then explicitly supplied CLI options. The default global threshold is `8`,
and a gate fails only when a measured score is strictly greater than its
effective global or path threshold. `--config PATH` selects a configuration
explicitly; its file must remain inside `--project-root`.

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

## Reports

Text reports are intended for terminals. JSON stdout is deterministic and
contains no timestamps or child-process noise. Single-project reports use
[schema v1](../schemas/report-v1.schema.json); package-group reports use
[schema v2](../schemas/report-v2.schema.json), with each row qualified by its
group. `npm run check:schemas` validates both supported shapes.

The process exits with `0` when analysis completes within policy, `1` for
invalid input, configuration, parsing, execution, or missing-evidence errors,
and `2` when a measured CRAP score is strictly greater than its effective
threshold. `--report-only` keeps unknown rows visible but cannot give them a
numeric score.


## Platforms and limitations

Keep npm optional dependencies enabled: the launcher needs the matching native
package. A missing package or unsupported platform produces an actionable error.
Unsupported platforms can build the standalone binary from source; see
[development instructions](./how-its-made.md#development).

The npm package is a launcher plus one optional native package selected by
Node's `process.platform` and `process.arch`; it performs no postinstall
download. Supported npm targets are Linux x64/arm64, macOS x64/arm64, and
Windows x64. Optional dependencies must remain enabled for normal npm
installation. The npm wrapper requires Node `>=20.19.0 <25` (Node 20, 22, or
24 LTS). The CLI analyzes TypeScript/TSX only; JavaScript, raw V8 coverage,
source-map reconstruction, SARIF/HTML, baseline ratchets, and changed-lines
gates are outside v1.


## Standalone archives

Every release publishes one archive per supported target. The names are
unambiguous and include the semantic version and target:

```text
crap4ts-1.0.0-linux-x64.tar.gz
crap4ts-1.0.0-linux-arm64.tar.gz
crap4ts-1.0.0-darwin-x64.tar.gz
crap4ts-1.0.0-darwin-arm64.tar.gz
crap4ts-1.0.0-win32-x64.tar.gz
```

Each archive contains the native executable, `LICENSE`, and this README.
`SHA256SUMS` covers exactly those five standalone archives plus the trusted
`BINARY-SHA256SUMS` record (six entries total). The npm directory contains
`npm/NPM-SHA256SUMS` for the six npm tarballs;
verify it before extracting an archive:

```sh
sha256sum -c SHA256SUMS
```

On Windows, use `Get-FileHash` and compare each SHA-256 value in
`SHA256SUMS` before extracting.

Linux binaries target glibc. Linux musl distributions and unsupported
operating-system/CPU combinations should build from the Cargo workspace. The
release target map is checked in at [`release-targets.json`](../release-targets.json)
so archive names, npm package metadata, and workflow targets cannot drift.


## Generated coverage on Windows

Generated commands use direct argv execution. Windows `.cmd` and `.bat` shims
are rejected; use a native executable such as `node.exe` with npm's CLI
JavaScript file. See `crap4ts --help` for all flags.
