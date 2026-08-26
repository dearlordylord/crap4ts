#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const targetDefinitions = JSON.parse(
  fs.readFileSync(path.join(root, 'release-targets.json'), 'utf8'),
);
const targets = Object.freeze(
  Object.fromEntries(
    Object.entries(targetDefinitions).map(([target, descriptor]) => [
      target,
      Object.freeze({ ...descriptor }),
    ]),
  ),
);

function usage() {
  return [
    'Usage:',
    '  node scripts/stage-platform.js --target <platform> --binary <path>',
    '  node scripts/stage-platform.js --binary-dir <directory>',
    '',
    `Platforms: ${Object.keys(targets).join(', ')}`,
    'The binary directory form expects <platform> (or crap4ts-<platform>) filenames.',
  ].join('\n');
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--help' || arg === '-h') {
      process.stdout.write(`${usage()}\n`);
      process.exit(0);
    }
    if (arg === '--target' || arg === '--binary' || arg === '--binary-dir') {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) {
        throw new Error(`${arg} requires a value\n\n${usage()}`);
      }
      options[arg.slice(2)] = value;
      index += 1;
      continue;
    }
    throw new Error(`unknown argument ${arg}\n\n${usage()}`);
  }
  if (options.target && options['binary-dir']) {
    throw new Error('--target cannot be combined with --binary-dir');
  }
  if (options.target && !options.binary) {
    throw new Error('--target requires --binary');
  }
  if (!options.target && !options['binary-dir']) {
    throw new Error(`provide --target/--binary or --binary-dir\n\n${usage()}`);
  }
  return options;
}

function binaryForDirectory(directory, target) {
  const descriptor = targets[target];
  if (!descriptor) {
    throw new Error(`unsupported target ${JSON.stringify(target)}; expected ${Object.keys(targets).join(', ')}`);
  }
  const names = [
    descriptor.binaryName,
    target,
    `crap4ts-${target}`,
  ];
  if (descriptor.binaryName.endsWith('.exe')) {
    names.push(`${target}.exe`, `crap4ts-${target}.exe`);
  }
  for (const name of names) {
    const candidate = path.join(directory, name);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    `missing prebuilt binary for ${target} in ${directory}; expected ${names.join(' or ')}`,
  );
}

function stage(target, source) {
  const descriptor = targets[target];
  if (!descriptor) {
    throw new Error(`unsupported target ${JSON.stringify(target)}; expected ${Object.keys(targets).join(', ')}`);
  }
  const packageRoot = path.join(root, 'packages', descriptor.packageDirectory);
  const packageJsonPath = path.join(packageRoot, 'package.json');
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
  if (packageJson.crap4tsBinary !== descriptor.binaryPath) {
    throw new Error(
      `${packageJsonPath} declares ${packageJson.crap4tsBinary || '<no crap4tsBinary>'}, expected ${descriptor.binaryPath}`,
    );
  }
  const sourcePath = path.resolve(source);
  const sourceStats = fs.lstatSync(sourcePath);
  if (sourceStats.isSymbolicLink()) {
    throw new Error(`prebuilt binary ${sourcePath} must not be a symlink`);
  }
  if (!sourceStats.isFile()) {
    throw new Error(`prebuilt binary ${sourcePath} is not a file`);
  }
  if (descriptor.os !== 'win32' && (sourceStats.mode & 0o111) === 0) {
    throw new Error(`prebuilt binary ${sourcePath} is not executable`);
  }
  const destination = path.join(packageRoot, descriptor.binaryPath);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.copyFileSync(sourcePath, destination);
  if (descriptor.os !== 'win32') {
    fs.chmodSync(destination, 0o755);
  }
  process.stdout.write(`staged ${target}: ${destination}\n`);
  return destination;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.target) {
    stage(options.target, options.binary);
    return;
  }
  const directory = path.resolve(options['binary-dir']);
  for (const target of Object.keys(targets)) {
    stage(target, binaryForDirectory(directory, target));
  }
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`platform staging failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { binaryForDirectory, parseArgs, stage, targets, usage };
