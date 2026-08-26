'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { npmBinInvocation, npmInvocation } = require('./npm-command.js');
const { verifyMetaPack } = require('./pack-platform.js');

test('Windows npm subprocesses use node plus npm_execpath instead of spawning npm.cmd', () => {
  assert.deepEqual(
    npmInvocation('win32', 'C:\\node\\node.exe', 'C:\\node\\npm-cli.js'),
    { command: 'C:\\node\\node.exe', argsPrefix: ['C:\\node\\npm-cli.js'] },
  );
});

test('POSIX npm subprocesses invoke npm directly', () => {
  assert.deepEqual(npmInvocation('linux', '/usr/bin/node', '/usr/lib/npm/npm-cli.js'), {
    command: 'npm',
    argsPrefix: [],
  });
});

test('Windows npm subprocesses fail closed without the lifecycle CLI path', () => {
  assert.throws(() => npmInvocation('win32', 'node.exe', ''), /npm_execpath/);
});

test('Windows npm bin shims run through the command interpreter', () => {
  assert.deepEqual(npmBinInvocation('C:\\tmp\\crap4ts.cmd', ['--help'], 'win32'), {
    command: 'C:\\tmp\\crap4ts.cmd',
    args: ['--help'],
    spawnOptions: { shell: true },
  });
});

test('Windows pack metadata may omit the launcher executable bit', () => {
  assert.doesNotThrow(() => verifyMetaPack({
    packageJson: {
      name: '@crap4ts/crap4ts',
      optionalDependencies: Object.fromEntries(
        Object.values(require('../release-targets.json')).map(({ packageName }) => [packageName, '1.0.0']),
      ),
    },
    metadata: { files: [{ path: 'bin/crap4ts.js', mode: 0o644 }] },
  }, 'win32'));
});
