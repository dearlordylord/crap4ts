#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');

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

function main() {
  const rustVersion = rustWorkspaceVersion();
  const mismatches = [];
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
  if (mismatches.length > 0) {
    throw new Error(
      `Rust workspace is ${rustVersion}, but npm package versions differ: ${mismatches.join(
        ', ',
      )}`,
    );
  }
  process.stdout.write(`crap4ts versions aligned at ${rustVersion}\n`);
}

try {
  main();
} catch (error) {
  process.stderr.write(`version check failed: ${error.message}\n`);
  process.exitCode = 1;
}
