#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const WRAPPER_METADATA = require('../package.json');

const SIGNALS = Object.freeze(['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT']);

function platformKey() {
  return `${process.platform}-${process.arch}`;
}

function supportedPlatformKeys(metadata = WRAPPER_METADATA) {
  return Object.keys(metadata.crap4tsBinaries || {});
}

function platformDescriptor(key, metadata = WRAPPER_METADATA) {
  const binaryPath = metadata.crap4tsBinaries?.[key];
  return binaryPath ? { binaryPath } : undefined;
}

function fail(message) {
  process.stderr.write(`crap4ts: ${message}\n`);
  process.exitCode = 1;
}

function signalExitCode(signal) {
  const signalNumber = os.constants.signals[signal];
  return typeof signalNumber === 'number' ? 128 + signalNumber : 1;
}

function main() {
  const key = platformKey();
  const selected = platformDescriptor(key);
  if (!selected) {
    fail(
      `unsupported platform ${JSON.stringify(key)}. Supported platforms: ${supportedPlatformKeys().join(
        ', ',
      )}. Install a supported package or build crap4ts with Cargo.`,
    );
    return;
  }

  const binaryPath = path.resolve(__dirname, '..', selected.binaryPath);
  let binaryStats;
  try {
    binaryStats = fs.statSync(binaryPath);
  } catch {
    fail(
      `bundled binary for ${key} is missing at ${binaryPath}. Reinstall the package or build crap4ts with Cargo.`,
    );
    return;
  }
  if (!binaryStats.isFile()) {
    fail(`bundled binary for ${key} is invalid at ${binaryPath}.`);
    return;
  }
  if (process.platform !== 'win32') {
    try {
      fs.accessSync(binaryPath, fs.constants.X_OK);
    } catch {
      fail(
        `bundled binary for ${key} is not executable at ${binaryPath}. Reinstall the package.`,
      );
      return;
    }
  }

  const child = spawn(binaryPath, process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: false,
  });
  let settled = false;

  const removeSignalHandlers = () => {
    for (const signal of SIGNALS) {
      process.removeListener(signal, forwardSignal);
    }
  };

  const forwardSignal = (signal) => {
    if (settled || child.exitCode !== null) {
      return;
    }
    try {
      child.kill(signal);
    } catch {
      // The child may have exited between the check and kill call. Its exit
      // event remains the source of truth for the wrapper's final status.
    }
  };

  for (const signal of SIGNALS) {
    process.on(signal, forwardSignal);
  }

  child.once('error', (error) => {
    if (settled) {
      return;
    }
    settled = true;
    removeSignalHandlers();
    if (error && error.code === 'EACCES') {
      fail(`unable to execute platform binary ${binaryPath}: permission denied.`);
    } else if (error && error.code === 'ENOENT') {
      fail(`platform binary ${binaryPath} could not be started; reinstall the platform package.`);
    } else {
      fail(`unable to start platform binary ${binaryPath}: ${error.message}`);
    }
  });

  child.once('exit', (code, signal) => {
    if (settled) {
      return;
    }
    settled = true;
    removeSignalHandlers();
    process.exitCode = signal ? signalExitCode(signal) : code === null ? 1 : code;
  });
}

if (require.main === module) main();

module.exports = { main, platformDescriptor, supportedPlatformKeys };
