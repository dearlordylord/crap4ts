#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const SUPPORTED_PLATFORMS = Object.freeze({
  'linux-x64': Object.freeze({
    packageName: '@crap4ts/linux-x64',
    binaryName: 'crap4ts',
  }),
  'linux-arm64': Object.freeze({
    packageName: '@crap4ts/linux-arm64',
    binaryName: 'crap4ts',
  }),
  'darwin-x64': Object.freeze({
    packageName: '@crap4ts/darwin-x64',
    binaryName: 'crap4ts',
  }),
  'darwin-arm64': Object.freeze({
    packageName: '@crap4ts/darwin-arm64',
    binaryName: 'crap4ts',
  }),
  'win32-x64': Object.freeze({
    packageName: '@crap4ts/win32-x64',
    binaryName: 'crap4ts.exe',
  }),
});

const SIGNALS = Object.freeze(['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGQUIT']);

function platformKey() {
  return `${process.platform}-${process.arch}`;
}

function fail(message) {
  process.stderr.write(`crap4ts: ${message}\n`);
  process.exitCode = 1;
}

function packageVersion(packageJsonPath) {
  try {
    const metadata = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
    return metadata.version;
  } catch {
    return undefined;
  }
}

function signalExitCode(signal) {
  const signalNumber = os.constants.signals[signal];
  return typeof signalNumber === 'number' ? 128 + signalNumber : 1;
}

function main() {
  const key = platformKey();
  const selected = SUPPORTED_PLATFORMS[key];
  if (!selected) {
    fail(
      `unsupported platform ${JSON.stringify(key)}. Supported platforms: ${Object.keys(
        SUPPORTED_PLATFORMS,
      ).join(', ')}. Install a supported package or build crap4ts with Cargo.`,
    );
    return;
  }

  const packageJsonRequest = `${selected.packageName}/package.json`;
  let packageJsonPath;
  try {
    packageJsonPath = require.resolve(packageJsonRequest, { paths: [__dirname] });
  } catch {
    fail(
      `platform package ${selected.packageName}@${packageVersion(
        path.resolve(__dirname, '..', 'package.json'),
      ) || 'the installed version'} is missing for ${key}. Reinstall crap4ts with optional dependencies enabled.`,
    );
    return;
  }

  const wrapperVersion = packageVersion(path.resolve(__dirname, '..', 'package.json'));
  const binaryVersion = packageVersion(packageJsonPath);
  if (wrapperVersion && binaryVersion && wrapperVersion !== binaryVersion) {
    fail(
      `version mismatch: npm wrapper is ${wrapperVersion}, but ${selected.packageName} is ${binaryVersion}. Install matching crap4ts packages.`,
    );
    return;
  }

  const binaryPath = path.join(path.dirname(packageJsonPath), 'bin', selected.binaryName);
  let binaryStats;
  try {
    binaryStats = fs.statSync(binaryPath);
  } catch {
    fail(
      `platform package ${selected.packageName} is installed but its binary is missing at ${binaryPath}. Reinstall the package or build crap4ts with Cargo.`,
    );
    return;
  }
  if (!binaryStats.isFile()) {
    fail(`platform package ${selected.packageName} has an invalid binary at ${binaryPath}.`);
    return;
  }
  if (process.platform !== 'win32') {
    try {
      fs.accessSync(binaryPath, fs.constants.X_OK);
    } catch {
      fail(
        `platform package ${selected.packageName} contains a non-executable binary at ${binaryPath}. Reinstall the package.`,
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

main();
