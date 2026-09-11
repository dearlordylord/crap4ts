#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const binary = path.resolve(process.argv[2] || '');
assert.ok(process.argv[2], 'Usage: node scripts/linux-compat-smoke.js BINARY');

// Inspect the actual ELF, so changing runners cannot silently raise the ABI floor.
const elf = spawnSync('readelf', ['--version-info', binary], { encoding: 'utf8' });
assert.equal(elf.status, 0, elf.error?.message || elf.stderr);
const versions = [...elf.stdout.matchAll(/\bGLIBC_(\d+)\.(\d+)(?:\.(\d+))?\b/g)]
  .map((match) => match.slice(1).map((part) => Number(part || 0)));
assert.ok(versions.length > 0, 'expected a dynamically linked glibc binary');
for (const [major, minor, patch] of versions) {
  assert.ok(major < 2 || (major === 2 && (minor < 35 || (minor === 35 && patch === 0))),
    `binary requires GLIBC_${major}.${minor}.${patch}; maximum supported requirement is GLIBC_2.35`);
}

// Run the exact release payload, without rebuilding it in the compatibility image.
const image = 'debian:12-slim';
const pull = spawnSync('docker', ['pull', image], { stdio: 'inherit' });
assert.equal(pull.status, 0, pull.error?.message || 'could not pull Debian 12');
const fixture = path.join(root, 'crates/crap4ts-cli/tests/fixtures/issue-nine');
function run(args, cwd = '/fixture', status = 0) {
  const result = spawnSync('docker', [
    'run', '--rm', '--network', 'none',
    '--mount', `type=bind,source=${binary},target=/usr/local/bin/crap4ts,readonly`,
    '--mount', `type=bind,source=${fixture},target=/fixture,readonly`,
    '--workdir', cwd, image, '/usr/local/bin/crap4ts', ...args,
  ], { encoding: 'utf8' });
  assert.equal(result.status, status, result.error?.message || result.stderr);
  return result.stdout;
}
assert.equal(run(['--version']).trim(), `crap4ts ${require('../package.json').version}`);
assert.match(run(['--help']), /Usage: crap4ts/);
const single = JSON.parse(run(['--coverage', 'coverage-final.json', 'src', '--format', 'json'], '/fixture/packages/istanbul'));
assert.equal(single.version, 1);
assert.ok(single.rows.length > 0);
assert.ok(single.rows.every((row) => row.coverage.status === 'measured'));
const grouped = JSON.parse(run(['--format', 'json']));
assert.equal(grouped.version, 2);
assert.deepEqual(grouped.groups.map((group) => group.name).sort(), ['istanbul', 'lcov']);
assert.ok(grouped.rows.length > 0);
assert.ok(grouped.rows.every((row) => row.coverage.status === 'measured'));
run(['--coverage', 'coverage-final.json', 'src', '--threshold', '0'], '/fixture/packages/istanbul', 2);

// Install the packed npm launcher and native package on the same Debian baseline.
const npmImage = 'node:22-bookworm-slim';
const npmPull = spawnSync('docker', ['pull', npmImage], { stdio: 'inherit' });
assert.equal(npmPull.status, 0, npmPull.error?.message || 'could not pull Debian 12 Node image');
const npmSmoke = spawnSync('docker', [
  'run', '--rm', '--network', 'none',
  '--mount', `type=bind,source=${root},target=/workspace`,
  '--mount', `type=bind,source=${binary},target=/release/crap4ts,readonly`,
  '--workdir', '/workspace', npmImage,
  'node', 'scripts/npm-smoke.js', '--binary', '/release/crap4ts',
], { stdio: 'inherit' });
assert.equal(npmSmoke.status, 0, npmSmoke.error?.message || 'Debian 12 npm installation smoke failed');
console.log('Linux release binary passes glibc <=2.35 audit and Debian 12 version/help/Istanbul/LCOV/gate smoke');
