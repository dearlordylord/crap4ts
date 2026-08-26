#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const crypto = require('node:crypto');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { targets, stage, binaryForDirectory } = require('./stage-platform.js');

const root = path.resolve(__dirname, '..');

function usage() {
  return [
    'Usage:',
    '  node scripts/pack-platform.js --target <platform> --binary <path> [--output-dir <dir>]',
    '  node scripts/pack-platform.js --binary-dir <dir> [--output-dir <dir>]',
    '',
    `Platforms: ${Object.keys(targets).join(', ')}`,
  ].join('\n');
}

function parseArgs(args) {
  const options = { outputDir: path.join(root, 'dist', 'npm') };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--help' || arg === '-h') {
      process.stdout.write(`${usage()}\n`);
      process.exit(0);
    }
    if (['--target', '--binary', '--binary-dir', '--output-dir'].includes(arg)) {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) {
        throw new Error(`${arg} requires a value\n\n${usage()}`);
      }
      const optionName = arg.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
      options[optionName] = value;
      index += 1;
      continue;
    }
    throw new Error(`unknown argument ${arg}\n\n${usage()}`);
  }
  if (options.target && options.binaryDir) {
    throw new Error('--target cannot be combined with --binary-dir');
  }
  if (options.target && !options.binary) {
    throw new Error('--target requires --binary');
  }
  if (!options.target && !options.binaryDir) {
    throw new Error(`provide --target/--binary or --binary-dir\n\n${usage()}`);
  }
  return options;
}

function packageMetadata(packageDirectory) {
  return JSON.parse(
    fs.readFileSync(path.join(root, 'packages', packageDirectory, 'package.json'), 'utf8'),
  );
}

function removeStaged(target) {
  const descriptor = targets[target];
  if (!descriptor) return;
  const destination = path.join(
    root,
    'packages',
    descriptor.packageDirectory,
    descriptor.binaryPath,
  );
  try {
    const metadata = fs.lstatSync(destination);
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      throw new Error(`staged package payload ${destination} is not a regular file`);
    }
    fs.rmSync(destination, { force: true });
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
}

function pack(packageDirectory, outputDir) {
  const packageJson = packageMetadata(packageDirectory);
  const result = spawnSync(
    process.platform === 'win32' ? 'npm.cmd' : 'npm',
    ['pack', '--ignore-scripts', '--json', '--pack-destination', outputDir],
    { cwd: path.join(root, 'packages', packageDirectory), encoding: 'utf8' },
  );
  if (result.error || result.status !== 0) {
    throw new Error(`npm pack failed for ${packageDirectory}: ${result.error?.message || result.stderr}`);
  }
  let packed;
  try {
    packed = JSON.parse(result.stdout)[0];
  } catch {
    throw new Error(`npm pack returned invalid metadata for ${packageDirectory}`);
  }
  const archive = path.join(outputDir, packed.filename);
  if (!fs.existsSync(archive)) {
    throw new Error(`npm pack did not produce ${archive}`);
  }
  if (packed.name !== undefined && packed.name !== packageJson.name) {
    throw new Error(`npm pack returned ${packed.name} for ${packageJson.name}`);
  }
  if (packed.version !== undefined && packed.version !== packageJson.version) {
    throw new Error(`npm pack returned ${packed.version} for ${packageJson.name}`);
  }
  return { archive, metadata: packed, packageJson };
}

function verifyNativePack(result, target, source) {
  const descriptor = targets[target];
  if (!descriptor) throw new Error(`unsupported target ${JSON.stringify(target)}`);
  if (result.packageJson.name !== descriptor.packageName) {
    throw new Error(`${target} package metadata name is ${result.packageJson.name}`);
  }
  if (result.packageJson.crap4tsBinary !== descriptor.binaryPath) {
    throw new Error(`${target} package binary declaration does not match target map`);
  }
  const binaryPath = result.packageJson.crap4tsBinary;
  const file = result.metadata.files.find((entry) => entry.path === binaryPath);
  if (!file) {
    throw new Error(`${result.packageJson.name} tarball does not contain ${binaryPath}`);
  }
  if (descriptor.os !== 'win32' && (file.mode & 0o111) === 0) {
    throw new Error(`${result.packageJson.name} tarball payload ${binaryPath} is not executable`);
  }
  if (source) {
    const sourcePath = path.resolve(source);
    const sourceSize = fs.statSync(sourcePath).size;
    if (file.size !== sourceSize) {
      throw new Error(
        `${result.packageJson.name} payload size ${file.size} does not match ${sourceSize}`,
      );
    }
    const packed = spawnSync(
      'tar',
      ['-xOf', result.archive, `package/${binaryPath}`],
      { encoding: 'buffer', maxBuffer: 256 * 1024 * 1024 },
    );
    if (packed.error || packed.status !== 0) {
      throw new Error(`${result.packageJson.name} payload could not be read back from its tarball`);
    }
    const sourceDigest = crypto.createHash('sha256').update(fs.readFileSync(sourcePath)).digest('hex');
    const packedDigest = crypto.createHash('sha256').update(packed.stdout).digest('hex');
    if (packedDigest !== sourceDigest) {
      throw new Error(`${result.packageJson.name} payload bytes do not match the staged binary`);
    }
  }
  return file;
}

function verifyMetaPack(result) {
  if (result.packageJson.name !== '@crap4ts/crap4ts') {
    throw new Error(`expected @crap4ts/crap4ts meta-package, received ${result.packageJson.name}`);
  }
  const launcher = result.metadata.files.find((entry) => entry.path === 'bin/crap4ts.js');
  if (!launcher || (launcher.mode & 0o111) === 0) {
    throw new Error('crap4ts meta-package tarball does not contain executable bin/crap4ts.js');
  }
  const expectedTargets = Object.values(targets).map((descriptor) => descriptor.packageName).sort();
  const actualTargets = Object.keys(result.packageJson.optionalDependencies || {}).sort();
  if (JSON.stringify(actualTargets) !== JSON.stringify(expectedTargets)) {
    throw new Error(
      `crap4ts optional dependencies do not match target map: ${actualTargets.join(', ')}`,
    );
  }
  return launcher;
}

function packTargets(sources, outputDir, selectedTargets = Object.keys(targets)) {
  fs.mkdirSync(outputDir, { recursive: true });
  const staged = [];
  try {
    const packages = selectedTargets.map((target) => {
      const source = sources[target];
      if (!source) throw new Error(`missing source binary for ${target}`);
      stage(target, source);
      staged.push(target);
      const result = pack(targets[target].packageDirectory, outputDir);
      verifyNativePack(result, target, source);
      return result;
    });
    const meta = pack('crap4ts', outputDir);
    verifyMetaPack(meta);
    return { packages, meta };
  } finally {
    for (const target of staged) removeStaged(target);
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const outputDir = path.resolve(options.outputDir);
  fs.mkdirSync(outputDir, { recursive: true });
  const selectedTargets = options.target ? [options.target] : Object.keys(targets);
  const sources = Object.fromEntries(selectedTargets.map((target) => {
    if (!targets[target]) throw new Error(`unsupported target ${JSON.stringify(target)}`);
    return [target, options.binaryDir
      ? binaryForDirectory(path.resolve(options.binaryDir), target)
      : options.binary];
  }));
  const result = packTargets(sources, outputDir, selectedTargets);
  process.stdout.write(`${[...result.packages, result.meta].map((entry) => entry.archive).join('\n')}\n`);
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`npm packaging failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  main,
  pack,
  packTargets,
  parseArgs,
  removeStaged,
  verifyMetaPack,
  verifyNativePack,
};
