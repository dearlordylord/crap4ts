# Release status of the inspirational `crap4ts` repositories

Date: 2026-08-26 (America/Montreal)

## Research question

How many of the 11 repositories reviewed in
[`2026-08-25-alternative-implementations.md`](./2026-08-25-alternative-implementations.md)
have actually been released, and does `dearlordylord/crap4ts` still need a real
v1 release?

## Definitions and method

This review distinguishes three kinds of evidence:

- **Formal GitHub release:** a non-draft object shown by the repository's
  GitHub Releases endpoint/page. A tag by itself is not a release.
- **Registry-published package:** a package version returned by the owning
  ecosystem registry and attributable to the repository. A version declared in
  `package.json`, `Cargo.toml`, or `pyproject.toml` is not publication evidence.
- **Not released:** neither of the preceding forms of publication was found.

GitHub repositories, Releases, and tags; the npm registry; GitHub Packages;
crates.io; and PyPI were checked on 2026-08-26. All evidence is first-party.

## Results

| Repository | Formal GitHub releases | Tags when no release | Registry publication | Classification |
|---|---|---|---|---|
| [`icaruswings/crap4ts`](https://github.com/icaruswings/crap4ts) | [0](https://github.com/icaruswings/crap4ts/releases) | [0](https://github.com/icaruswings/crap4ts/tags) | None. Its [`package.json`](https://github.com/icaruswings/crap4ts/blob/main/package.json) marks the package private. | Not released |
| [`alperlabs/crap4ts`](https://github.com/alperlabs/crap4ts) | [2; latest `v0.2.1`](https://github.com/alperlabs/crap4ts/releases/tag/v0.2.1) | — | [npm `@alperlabs/crap4ts@0.2.1`](https://registry.npmjs.org/@alperlabs%2Fcrap4ts/0.2.1) | GitHub release and registry package |
| [`breezy-bays-labs/crap4ts`](https://github.com/breezy-bays-labs/crap4ts) | [5 objects](https://github.com/breezy-bays-labs/crap4ts/releases), of which four are version releases; latest version release [`v1.0.1`](https://github.com/breezy-bays-labs/crap4ts/releases/tag/v1.0.1) | — | [npm `crap4ts@1.0.1`](https://registry.npmjs.org/crap4ts/1.0.1), whose metadata identifies this repository. Later `2.0.0-rc.*` versions identify a different repository, `breezy-bays-labs/crap-rs`, and are not counted here. | GitHub release and registry package |
| [`gligorkot/crap4ts`](https://github.com/gligorkot/crap4ts) | [3; latest version `v1.0.0`](https://github.com/gligorkot/crap4ts/releases/tag/v1.0.0) (the later-created `v1` object is an alias-style release) | — | [npm `@gligor/crap4ts@1.0.0`](https://registry.npmjs.org/@gligor%2Fcrap4ts/1.0.0) | GitHub release and registry package |
| [`danibram/crap4ts`](https://github.com/danibram/crap4ts) | [6; latest `v0.6.0`](https://github.com/danibram/crap4ts/releases/tag/v0.6.0) | — | [npm `@danibram/crap4ts@0.6.0`](https://registry.npmjs.org/@danibram%2Fcrap4ts/0.6.0) | GitHub release and registry package |
| [`Jaseempk/crap4ts`](https://github.com/Jaseempk/crap4ts) | [0](https://github.com/Jaseempk/crap4ts/releases) | [`v0.2.0`](https://github.com/Jaseempk/crap4ts/tags) | None attributable. Its manifest uses the unscoped `crap4ts` name, which the npm registry attributes to Breezy's repository. | Tag only; not released by the definitions above |
| [`fine405/crap4ts`](https://github.com/fine405/crap4ts) | [0](https://github.com/fine405/crap4ts/releases) | [0](https://github.com/fine405/crap4ts/tags) | None attributable. The [crates.io `crap4ts` record](https://crates.io/api/v1/crates/crap4ts) identifies `breezy-bays-labs/crap-rs`, not this repository. | Not released |
| [`sebassdc/crap4ts`](https://github.com/sebassdc/crap4ts) | [0](https://github.com/sebassdc/crap4ts/releases) | [`v0.1.1`](https://github.com/sebassdc/crap4ts/tags) | [npm `@sebassdc/crap4ts@0.1.1`](https://registry.npmjs.org/@sebassdc%2Fcrap4ts/0.1.1) | Registry package and tag, but no GitHub release |
| [`reaganthomas/crap4ts`](https://github.com/reaganthomas/crap4ts) | [0](https://github.com/reaganthomas/crap4ts/releases) | [0](https://github.com/reaganthomas/crap4ts/tags) | None attributable. Its manifest uses the npm name already attributed to Breezy's repository. | Not released |
| [`graffhyrum/crap4ts`](https://github.com/graffhyrum/crap4ts) | [1; `v1.1.0`](https://github.com/graffhyrum/crap4ts/releases/tag/v1.1.0) | — | [GitHub Packages `@graffhyrum/crap4ts@1.1.0`](https://github.com/graffhyrum/crap4ts/pkgs/npm/crap4ts) | GitHub release and registry package |
| [`lukasa1993/crap4ts`](https://github.com/lukasa1993/crap4ts) | [0](https://github.com/lukasa1993/crap4ts/releases) | [0](https://github.com/lukasa1993/crap4ts/tags) | None. The declared `crap4ts` Python project has [no PyPI project record](https://pypi.org/pypi/crap4ts/json) (404 when checked). | Not released |

## Counts

- **5 of 11 repositories** have at least one formal GitHub release, comprising
  17 GitHub release objects in total. One of Breezy's five is a maintenance
  announcement rather than a version release.
- **6 of 11 repositories** have an attributable registry-published package:
  five on the public npm registry and one on GitHub Packages.
- The union is **6 of 11 released through at least one of those channels**.
- **5 of 11 are not released** under the definitions above.
- Two repositories have tags but no formal GitHub release; only one of those,
  Sebassdc, also has a registry package.

## Does this repository need a release?

Yes—but it is not ready to publish at the reviewed commit.

Issue [#11](https://github.com/dearlordylord/crap4ts/issues/11) says to
"Release a production-ready v1 installable as a checksummed standalone binary
or through npm." The repository currently has [no GitHub release](https://github.com/dearlordylord/crap4ts/releases)
and [no tag](https://github.com/dearlordylord/crap4ts/tags). Implementing release
automation is necessary preparation, but it does not fulfill that shipment
language by itself.

Four blockers or release prerequisites should be resolved before creating
`v1.0.0`:

1. The first pushed [`master` CI run failed](https://github.com/dearlordylord/crap4ts/actions/runs/32961049617).
   Windows exposed both a path-identity test failure and a platform-dependent
   `Cargo.lock` parser failure in the version check. A release should start only
   from a green commit across the promised matrix.
2. The planned npm meta-package is named `crap4ts`, but
   [npm already assigns that name to Breezy's package](https://registry.npmjs.org/crap4ts/1.0.1).
   The exact `1.0.0` version is not present, but npm package ownership—not an
   unused version number—controls who can publish. npm package names are
   registry-global, so this workflow cannot publish the current meta-package.
   Use an owned scoped identity such as
   `@dearlordylord/crap4ts` and owned names for all five native packages, then
   update dependency selection, documentation, tests, and trusted-publisher
   configuration consistently.
3. The five planned `@crap4ts/*` native packages also return 404 from the npm
   registry. npm's [`npm trust` documentation](https://docs.npmjs.com/cli/v11/commands/npm-trust/)
   states that a package must already exist before a trusted publisher can be
   configured. The six renamed packages therefore need an authenticated,
   maintainer-controlled first-publication bootstrap. Configure the
   `release.yml` GitHub trusted publisher for each package only after that
   bootstrap; the current OIDC-only workflow cannot create brand-new package
   identities by itself.
4. After those changes, rerun CI, create a signed or annotated `v1.0.0` tag at
   the green `master` commit, let the existing fail-closed workflow build and
   verify all artifacts, and publish the npm packages plus the GitHub release.
   Verify the resulting registry metadata, release assets, and checksums from
   clean installations before treating #11 as complete.

The appropriate bookkeeping is therefore to reopen #11 (or create a focused
release-blocker issue linked to it) until a real release exists. Shipping is
justified by the project's stated acceptance criteria, not merely because six
other implementations shipped.
