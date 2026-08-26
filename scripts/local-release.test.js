'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const targets = require('../release-targets.json');
const { validatedUrl } = require('./download.js');
const {
  npmPublicationPlan,
  packageTarballName,
  isMissingReleaseError,
  releaseCommandInvocation,
  requireDraftForUpload,
  selectReleaseRun,
  selectSuccessfulCi,
} = require('./local-release.js');

test('local release publishes all native packages before the scoped meta-package', () => {
  const plan = npmPublicationPlan('1.2.3');
  const nativeNames = Object.values(targets).map(({ packageName }) => packageName).sort();

  assert.deepEqual(plan.map(({ name }) => name), [...nativeNames, '@crap4ts/crap4ts']);
  assert.deepEqual(
    plan.map(({ tarball }) => tarball),
    [...nativeNames, '@crap4ts/crap4ts'].map((name) => packageTarballName(name, '1.2.3')),
  );
});

test('scoped npm package identities map to npm pack archive names', () => {
  assert.equal(packageTarballName('@crap4ts/linux-x64', '1.0.0'), 'crap4ts-linux-x64-1.0.0.tgz');
  assert.equal(packageTarballName('@crap4ts/crap4ts', '1.0.0'), 'crap4ts-crap4ts-1.0.0.tgz');
});

test('workflow selection ignores runs belonging to a different commit', () => {
  const runs = [
    { databaseId: 1, headSha: 'old', headBranch: 'v1.0.0', status: 'completed', conclusion: 'success', event: 'push' },
    { databaseId: 2, headSha: 'wanted', headBranch: 'master', status: 'completed', conclusion: 'success', event: 'push' },
    { databaseId: 3, headSha: 'wanted', headBranch: 'v1.0.0', status: 'completed', conclusion: 'success', event: 'push' },
  ];
  assert.equal(selectSuccessfulCi(runs, 'wanted').databaseId, 2);
  assert.equal(selectReleaseRun(runs, 'wanted', 'v1.0.0').databaseId, 3);
  assert.equal(selectSuccessfulCi(runs, 'missing'), undefined);
});

test('only an actual missing GitHub release permits creation', () => {
  assert.equal(isMissingReleaseError({ status: 1, stderr: 'release not found' }), true);
  assert.equal(isMissingReleaseError({ status: 1, stderr: 'HTTP 404: Not Found' }), true);
  assert.equal(isMissingReleaseError({ status: 1, stderr: 'authentication failed' }), false);
});

test('missing assets cannot be added to an already-published GitHub release', () => {
  assert.doesNotThrow(() => requireDraftForUpload(true, 'SHA256SUMS'));
  assert.throws(() => requireDraftForUpload(false, 'SHA256SUMS'), /already published/);
});

test('local release delegates npm through the actual Windows package manager CLI', () => {
  assert.deepEqual(releaseCommandInvocation(
    'npm',
    ['whoami'],
    'win32',
    'C:\\node\\node.exe',
    'C:\\pnpm\\pnpm.cjs',
  ), {
    command: 'C:\\node\\node.exe',
    args: ['C:\\pnpm\\pnpm.cjs', 'exec', 'npm', 'whoami'],
    spawnOptions: {},
  });
});

test('release downloads require HTTPS', () => {
  assert.equal(validatedUrl('https://registry.npmjs.org/package.tgz').protocol, 'https:');
  assert.throws(() => validatedUrl('http://registry.npmjs.org/package.tgz'), /HTTPS/);
});
