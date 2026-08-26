'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { npmInvocation } = require('./npm-command.js');

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
