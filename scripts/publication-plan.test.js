'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { classify, assertSafe, digest } = require('./publication-plan.js');
const { parseArgs } = require('./npm-smoke.js');
test('publication plan distinguishes absent, identical, and conflicting assets', () => {
  const files = [{ name: 'a', path: __filename }];
  assert.equal(classify(files, {})[0].action, 'publish');
  assert.equal(classify(files, { a: digest(__filename) })[0].action, 'skip-identical');
  assert.throws(() => assertSafe(classify(files, { a: '0'.repeat(64) })), /conflicts/);
});

test('npm smoke maps release-dir to assembled release mode', () => {
  assert.equal(parseArgs(['--release-dir', 'dist/release', '--binary', 'dist/binaries/x']).releaseDir, 'dist/release');
});
