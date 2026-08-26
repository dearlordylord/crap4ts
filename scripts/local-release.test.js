'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const targets = require('../release-targets.json');
const {
  npmPublicationPlan,
  packageTarballName,
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
    { databaseId: 1, headSha: 'old', status: 'completed', conclusion: 'success', event: 'push' },
    { databaseId: 2, headSha: 'wanted', status: 'completed', conclusion: 'success', event: 'push' },
  ];
  assert.equal(selectSuccessfulCi(runs, 'wanted').databaseId, 2);
  assert.equal(selectReleaseRun(runs, 'wanted').databaseId, 2);
  assert.equal(selectSuccessfulCi(runs, 'missing'), undefined);
});
