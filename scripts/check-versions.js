#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const targetDefinitions = JSON.parse(
  fs.readFileSync(path.join(root, 'release-targets.json'), 'utf8'),
);

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function rustWorkspaceVersion() {
  const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const lines = cargo.split(/\r?\n/);
  const section = lines.indexOf('[workspace.package]');
  const nextSection = section < 0
    ? -1
    : lines.slice(section + 1).findIndex((line) => line.startsWith('['));
  const sectionEnd = nextSection < 0 ? lines.length : section + 1 + nextSection;
  const sectionLines = section < 0 ? [] : lines.slice(section + 1, sectionEnd);
  const version = sectionLines
    .join('\n')
    .match(/^version\s*=\s*"([^"]+)"\s*$/m);
  if (!version) {
    throw new Error('Cargo.toml has no [workspace.package] version');
  }
  return version[1];
}

function rustManifestVersions() {
  const crateRoot = path.join(root, 'crates');
  const entries = fs.readdirSync(crateRoot, { withFileTypes: true });
  const versions = new Map();
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const manifestPath = path.join(crateRoot, entry.name, 'Cargo.toml');
    if (!fs.existsSync(manifestPath)) continue;
    const manifest = fs.readFileSync(manifestPath, 'utf8');
    const name = manifest.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    const version = manifest.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
    const workspaceVersion = /^version\.workspace\s*=\s*true\s*$/m.test(manifest);
    if (name && version) versions.set(name, version);
    else if (name && workspaceVersion) versions.set(name, rustWorkspaceVersion());
  }
  return versions;
}

function npmPackageFiles() {
  const packageRoot = path.join(root, 'packages');
  const entries = fs.existsSync(packageRoot) ? fs.readdirSync(packageRoot, { withFileTypes: true }) : [];
  return [
    path.join(root, 'package.json'),
    ...entries
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(packageRoot, entry.name, 'package.json'))
      .filter((file) => fs.existsSync(file)),
  ];
}

function lockfileVersions() {
  const lockPath = path.join(root, 'package-lock.json');
  if (!fs.existsSync(lockPath)) {
    throw new Error('package-lock.json is missing');
  }
  const lock = readJson(lockPath);
  const versions = new Map();
  for (const [name, metadata] of Object.entries(lock.packages || {})) {
    if (!name.startsWith('packages/') || !metadata.version) continue;
    const packageJson = readJson(path.join(root, name, 'package.json'));
    versions.set(packageJson.name, metadata.version);
  }
  return versions;
}

function lockfileRoot() {
  const lock = readJson(path.join(root, 'package-lock.json'));
  return lock.packages?.[''] || {};
}

function cargoPackageVersions() {
  const lock = fs.readFileSync(path.join(root, 'Cargo.lock'), 'utf8');
  const expectedPackages = new Set(['crap4ts', 'crap4ts-core']);
  const versions = new Map();
  for (const block of lock.split(/\n\[\[package\]\]\n/).slice(1)) {
    const name = block.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    const version = block.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (name && version && expectedPackages.has(name)) versions.set(name, version);
  }
  return versions;
}

function reportSchemaVersions() {
  const domain = fs.readFileSync(
    path.join(root, 'crates', 'crap4ts-core', 'src', 'domain.rs'),
    'utf8',
  );
  const report = domain.match(/pub const REPORT_VERSION: u32 = (\d+);/);
  const aggregate = domain.match(/pub const AGGREGATE_REPORT_VERSION: u32 = REPORT_VERSION \+ (\d+);/);
  if (!report || !aggregate) {
    throw new Error('Rust report schema version constants are missing');
  }
  const versions = {
    v1: Number(report[1]),
    v2: Number(report[1]) + Number(aggregate[1]),
  };
  for (const [name, version] of Object.entries(versions)) {
    const schemaPath = path.join(root, 'schemas', `report-${name}.schema.json`);
    if (!fs.existsSync(schemaPath)) {
      throw new Error(`missing JSON schema ${path.relative(root, schemaPath)}`);
    }
    const schema = readJson(schemaPath);
    if (schema.properties?.version?.const !== version) {
      throw new Error(
        `${path.relative(root, schemaPath)} declares version ${schema.properties?.version?.const}, expected ${version}`,
      );
    }
  }
  return versions;
}

function checkBinaryVersion(binary, expected) {
  const result = spawnSync(binary, ['--version'], { cwd: root, encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw new Error(`binary version check failed: ${result.error?.message || result.stderr}`);
  }
  const match = result.stdout.trim().match(/^crap4ts\s+(\S+)$/);
  if (!match || match[1] !== expected) {
    throw new Error(`binary reports ${result.stdout.trim() || '<empty>'}, expected crap4ts ${expected}`);
  }
}

function validateTag(tag, expected) {
  if (tag !== `v${expected}`) {
    throw new Error(`release tag ${tag} does not exactly match v${expected}`);
  }
}

function main(options = {}) {
  const rustVersion = rustWorkspaceVersion();
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(rustVersion)) {
    throw new Error(`workspace version ${rustVersion} is not a semantic version`);
  }
  const mismatches = [];
  for (const [crate, version] of rustManifestVersions()) {
    if (version !== rustVersion) mismatches.push(`${crate} crate is ${version}`);
  }
  const metadataByName = new Map();
  for (const file of npmPackageFiles()) {
    const metadata = readJson(file);
    metadataByName.set(metadata.name, metadata);
    if (metadata.version !== rustVersion) {
      mismatches.push(`${path.relative(root, file)} is ${metadata.version || '<missing>'}`);
    }
  }
  const wrapper = metadataByName.get('crap4ts');
  for (const [name, version] of Object.entries(wrapper?.optionalDependencies || {})) {
    if (version !== rustVersion) {
      mismatches.push(`crap4ts optional dependency ${name} is ${version}`);
    }
    if (metadataByName.get(name)?.version !== rustVersion) {
      mismatches.push(`${name} package is not present at ${rustVersion}`);
    }
  }
  const locked = lockfileVersions();
  const lockedRoot = lockfileRoot();
  if (lockedRoot.version !== rustVersion) {
    mismatches.push(`package-lock.json root is ${lockedRoot.version || '<missing>'}`);
  }
  for (const metadata of metadataByName.values()) {
    if (metadata.name === 'crap4ts-workspace') continue;
    if (locked.get(metadata.name) !== metadata.version) {
      mismatches.push(
        `package-lock.json has ${metadata.name} at ${locked.get(metadata.name) || '<missing>'}`,
      );
    }
  }
  for (const [target, descriptor] of Object.entries(targetDefinitions)) {
    const packageMetadata = metadataByName.get(descriptor.packageName);
    if (!packageMetadata) {
      mismatches.push(`${target} package ${descriptor.packageName} is missing`);
      continue;
    }
    if (packageMetadata.crap4tsBinary !== descriptor.binaryPath) {
      mismatches.push(
        `${descriptor.packageName} declares ${packageMetadata.crap4tsBinary || '<missing>'}, expected ${descriptor.binaryPath}`,
      );
    }
    if (packageMetadata.os?.[0] !== descriptor.os || packageMetadata.cpu?.[0] !== descriptor.cpu) {
      mismatches.push(`${descriptor.packageName} platform metadata does not match ${target}`);
    }
  }
  const cargoLocked = cargoPackageVersions();
  for (const packageName of ['crap4ts', 'crap4ts-core']) {
    if (cargoLocked.get(packageName) !== rustVersion) {
      mismatches.push(
        `Cargo.lock has ${packageName} at ${cargoLocked.get(packageName) || '<missing>'}`,
      );
    }
  }
  const schemaVersions = reportSchemaVersions();
  if (options.binary) checkBinaryVersion(path.resolve(options.binary), rustVersion);
  if (options.tag) validateTag(options.tag, rustVersion);
  if (mismatches.length > 0) {
    throw new Error(
      `Rust workspace is ${rustVersion}, but npm package versions differ: ${mismatches.join(
        ', ',
      )}`,
    );
  }
  process.stdout.write(
    `crap4ts versions aligned at ${rustVersion} (JSON schemas v${schemaVersions.v1}/v${schemaVersions.v2})\n`,
  );
}

function parseArgs(args) {
  const options = {};
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--binary' || argument === '--tag') {
      const value = args[index + 1];
      if (!value) throw new Error(`${argument} requires a value`);
      options[argument.slice(2)] = value;
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      process.stdout.write('Usage: node scripts/check-versions.js [--binary PATH] [--tag vX.Y.Z]\n');
      return null;
    } else {
      throw new Error(`unknown argument ${argument}`);
    }
  }
  return options;
}

if (require.main === module) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options) main(options);
  } catch (error) {
    process.stderr.write(`version check failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  checkBinaryVersion,
  cargoPackageVersions,
  lockfileVersions,
  lockfileRoot,
  main,
  reportSchemaVersions,
  rustManifestVersions,
  rustWorkspaceVersion,
  validateTag,
};
