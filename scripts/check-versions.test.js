#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const fc = require('fast-check');
const { cargoPackageVersionsFromLock } = require('./check-versions.js');
const metaPackage = require('../packages/crap4ts/package.json');
const targets = require('../release-targets.json');

test('Cargo.lock package versions are independent of newline convention and unrelated blocks', () => {
  fc.assert(
    fc.property(
      fc.constantFrom('\n', '\r\n'),
      fc.array(
        fc.record({
          name: fc.stringMatching(/^dependency-[a-z]{1,12}$/),
          version: fc.tuple(fc.nat({ max: 20 }), fc.nat({ max: 20 }), fc.nat({ max: 20 }))
            .map((parts) => parts.join('.')),
        }),
        { maxLength: 12 },
      ),
      (newline, dependencies) => {
        const packages = [
          ...dependencies,
          { name: 'crap4ts', version: '1.0.0' },
          { name: 'crap4ts-core', version: '1.0.0' },
        ];
        const lock = [
          'version = 4',
          ...packages.flatMap(({ name, version }) => [
            '',
            '[[package]]',
            `name = "${name}"`,
            `version = "${version}"`,
          ]),
          '',
        ].join(newline);

        assert.deepEqual(
          [...cargoPackageVersionsFromLock(lock).entries()].sort(),
          [['crap4ts', '1.0.0'], ['crap4ts-core', '1.0.0']],
        );
      },
    ),
    { numRuns: 100 },
  );
});

test('public npm identities belong to the crap4ts organization', () => {
  assert.equal(metaPackage.name, '@crap4ts/crap4ts');
  assert.deepEqual(metaPackage.bin, { crap4ts: 'bin/crap4ts.js' });
  assert.deepEqual(
    Object.keys(metaPackage.optionalDependencies).sort(),
    Object.values(targets).map((target) => target.packageName).sort(),
  );
  assert.ok(
    [metaPackage.name, ...Object.keys(metaPackage.optionalDependencies)]
      .every((name) => name.startsWith('@crap4ts/')),
  );
});
