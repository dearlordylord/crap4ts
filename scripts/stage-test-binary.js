#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const targets = Object.freeze({
  'linux-x64': Object.freeze({ packageDirectory: 'crap4ts-linux-x64', binaryName: 'crap4ts' }),
  'linux-arm64': Object.freeze({ packageDirectory: 'crap4ts-linux-arm64', binaryName: 'crap4ts' }),
  'darwin-x64': Object.freeze({ packageDirectory: 'crap4ts-darwin-x64', binaryName: 'crap4ts' }),
  'darwin-arm64': Object.freeze({ packageDirectory: 'crap4ts-darwin-arm64', binaryName: 'crap4ts' }),
  'win32-x64': Object.freeze({ packageDirectory: 'crap4ts-win32-x64', binaryName: 'crap4ts.exe' }),
});

function main() {
  const key = `${process.platform}-${process.arch}`;
  const target = targets[key];
  if (!target) {
    throw new Error(`cannot stage a test binary for unsupported platform ${key}`);
  }

  const cargo = process.platform === 'win32' ? 'cargo.exe' : 'cargo';
  const build = spawnSync(cargo, ['build', '-p', 'crap4ts'], {
    cwd: root,
    stdio: 'inherit',
  });
  if (build.error) {
    throw new Error(`unable to run Cargo: ${build.error.message}`);
  }
  if (build.status !== 0) {
    throw new Error(`Cargo build failed with status ${build.status}`);
  }

  const built = path.join(root, 'target', 'debug', target.binaryName);
  if (!fs.existsSync(built)) {
    throw new Error(`Cargo build did not produce ${built}`);
  }
  const destination = path.join(
    root,
    'packages',
    target.packageDirectory,
    'bin',
    target.binaryName,
  );
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(built, destination);
  if (process.platform !== 'win32') {
    fs.chmodSync(destination, 0o755);
  }
}

try {
  main();
} catch (error) {
  process.stderr.write(`test binary staging failed: ${error.message}\n`);
  process.exitCode = 1;
}
