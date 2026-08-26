#!/usr/bin/env node
'use strict';

// Release assembly and verification are deliberately separate from publishing.
// This module creates only files in the caller-provided output directory; the
// workflow decides whether a verified directory is uploaded or published.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { targets, binaryForDirectory } = require('./stage-platform.js');
const { packTargets } = require('./pack-platform.js');
const {
  main: checkVersions,
  rustWorkspaceVersion,
} = require('./check-versions.js');

const root = path.resolve(__dirname, '..');
const targetNames = Object.keys(targets);
const REQUIRED_TARGETS = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64'];
assert.deepEqual([...targetNames].sort(), REQUIRED_TARGETS, 'release target map must contain exactly the five supported targets');
for (const target of REQUIRED_TARGETS) {
  const descriptor = targets[target];
  assert.equal(descriptor.archiveExtension, 'tar.gz', `${target} archive extension must be tar.gz`);
  if (target.startsWith('linux-')) assert.equal(descriptor.libc, 'glibc', `${target} must declare glibc libc`);
}
const packageNames = [...targetNames.map((target) => targets[target].packageName), 'crap4ts'];

function usage() {
  return [
    'Usage:',
    '  node scripts/release.js assemble --binary-dir <dir> --output-dir <dir>',
    '  node scripts/release.js verify --release-dir <dir> [--require-smoke]',
    '',
    'assemble creates five standalone tar.gz archives, six npm packages, and SHA256SUMS.',
    'verify checks the exact target/package/checksum set before publication.',
  ].join('\n');
}

function parseArgs(args) {
  const command = args[0];
  if (command === '--help' || command === '-h' || !command) {
    process.stdout.write(`${usage()}\n`);
    return null;
  }
  if (!['assemble', 'verify'].includes(command)) throw new Error(`unknown release command ${command}`);
  const options = {
    command,
    binaryDir: path.join(root, 'dist', 'binaries'),
    outputDir: path.join(root, 'dist', 'release'),
    releaseDir: path.join(root, 'dist', 'release'),
    requireSmoke: false,
  };
  for (let index = 1; index < args.length; index += 1) {
    const argument = args[index];
    if (['--binary-dir', '--output-dir', '--release-dir'].includes(argument)) {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) throw new Error(`${argument} requires a value`);
      options[argument.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = value;
      index += 1;
    } else if (argument === '--require-smoke') {
      options.requireSmoke = true;
    } else if (argument === '--help' || argument === '-h') {
      process.stdout.write(`${usage()}\n`);
      return null;
    } else {
      throw new Error(`unknown argument ${argument}`);
    }
  }
  return options;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function ensureDirectory(directory) {
  fs.mkdirSync(directory, { recursive: true });
  const metadata = fs.lstatSync(directory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    throw new Error(`${directory} must be a real directory`);
  }
}

function ensureRegularFile(file, label) {
  let metadata;
  try {
    metadata = fs.lstatSync(file);
  } catch (error) {
    throw new Error(`missing ${label || 'file'} ${file}: ${error.message}`);
  }
  if (metadata.isSymbolicLink() || !metadata.isFile() || metadata.size === 0) {
    throw new Error(`${label || 'file'} ${file} must be a non-empty regular file`);
  }
  return metadata;
}

function binaryForTarget(binaryDirectory, target) {
  const source = binaryForDirectory(binaryDirectory, target);
  const descriptor = targets[target];
  const metadata = ensureRegularFile(source, `${target} binary`);
  if (descriptor.os !== 'win32' && (metadata.mode & 0o111) === 0) {
    throw new Error(`${target} binary ${source} is not executable`);
  }
  return source;
}

function versionedArchiveName(version, target) {
  return `crap4ts-${version}-${target}.tar.gz`;
}

function clearGeneratedFiles(directory, version) {
  ensureDirectory(directory);
  const names = [
    ...targetNames.map((target) => versionedArchiveName(version, target)),
    'SHA256SUMS',
  '.smoke.ok',
    'npm/SHA256SUMS',
  ];
  for (const name of names) {
    const file = path.join(directory, name);
    if (fs.existsSync(file)) fs.rmSync(file, { force: true });
  }
  const npmDirectory = path.join(directory, 'npm');
  if (fs.existsSync(npmDirectory)) {
    const metadata = fs.lstatSync(npmDirectory);
    if (metadata.isSymbolicLink() || !metadata.isDirectory()) {
      throw new Error(`${npmDirectory} must be a real directory`);
    }
    for (const entry of fs.readdirSync(npmDirectory)) {
      if (entry.endsWith('.tgz')) fs.rmSync(path.join(npmDirectory, entry), { force: true });
    }
  } else {
    fs.mkdirSync(npmDirectory, { recursive: true });
  }
}

function copyFile(source, destination, executable) {
  fs.copyFileSync(source, destination);
  if (executable) fs.chmodSync(destination, 0o755);
}

function createArchive(stagingRoot, target, archive) {
  const descriptor = targets[target];
  const folder = `crap4ts-${target}`;
  const folderPath = path.join(stagingRoot, folder);
  fs.mkdirSync(folderPath, { recursive: true });
  const binary = path.join(folderPath, descriptor.binaryName);
  ensureRegularFile(binary, `${target} staged binary`);
  copyFile(path.join(root, 'LICENSE'), path.join(folderPath, 'LICENSE'), false);
  copyFile(path.join(root, 'README.md'), path.join(folderPath, 'README.md'), false);
  // GNU tar is present on the Linux assembly runner. Keep metadata stable so
  // rerunning a release for the same inputs produces byte-identical archives.
  execFileSync('tar', [
    '--sort=name',
    '--mtime=@0',
    '--owner=0',
    '--group=0',
    '--numeric-owner',
    '-czf',
    archive,
    '-C',
    stagingRoot,
    folder,
  ], { stdio: 'pipe' });
  ensureRegularFile(archive, `${target} archive`);
}

function archiveEntries(archive) {
  const listing = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' });
  return listing
    .split(/\r?\n/)
    .filter(Boolean)
    .map((entry) => entry.replace(/\/$/, ''))
    .filter((entry) => entry.length > 0)
    .sort();
}

function assertArchiveHasOnlyRegularFiles(archive, expected) {
  const listing = execFileSync('tar', ['-tvzf', archive], { encoding: 'utf8' });
  for (const line of listing.split(/\r?\n/).filter(Boolean)) {
    const type = line[0];
    if (type !== 'd' && type !== '-') throw new Error(`${archive} contains a symlink or special archive entry`);
  }
  assert.deepEqual(archiveEntries(archive), expected, `${archive} contains unexpected or missing entries`);
}

function expectedArchiveEntries(target) {
  const descriptor = targets[target];
  return [
    `crap4ts-${target}`,
    `crap4ts-${target}/${descriptor.binaryName}`,
    `crap4ts-${target}/LICENSE`,
    `crap4ts-${target}/README.md`,
  ].sort();
}

function hashFile(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function distributableFiles(directory, version) {
  return targetNames.map((target) => path.join(directory, versionedArchiveName(version, target)));
}

function actualStandaloneArchives(directory) {
  return fs.readdirSync(directory)
    .filter((name) => name.endsWith('.tar.gz'))
    .map((name) => path.join(directory, name))
    .sort();
}

function writeManifest(files, manifest) {
  const lines = files
    .map((file) => `${hashFile(file)}  ${path.relative(path.dirname(manifest), file).replaceAll('\\', '/')}`)
    .sort();
  fs.writeFileSync(manifest, `${lines.join('\n')}\n`);
}

function readManifest(manifest) {
  const lines = fs.readFileSync(manifest, 'utf8').split(/\r?\n/).filter(Boolean);
  const entries = new Map();
  for (const line of lines) {
    const match = line.match(/^([a-f0-9]{64})  (.+)$/);
    if (!match || entries.has(match[2])) throw new Error(`malformed or duplicate checksum line: ${line}`);
    entries.set(match[2], match[1]);
  }
  return entries;
}

function packageJsonFromArchive(archive) {
  try {
    return JSON.parse(execFileSync('tar', ['-xOf', archive, 'package/package.json'], { encoding: 'utf8' }));
  } catch (error) {
    throw new Error(`package archive ${archive} has no readable package/package.json: ${error.message}`);
  }
}

function packageArchiveEntries(archive) {
  return execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
    .map((entry) => entry.replace(/\/$/, ''));
}

function findNpmArchive(directory, packageName, version) {
  const safeName = packageName.replace(/^@/, '').replace('/', '-');
  const expected = `${safeName}-${version}.tgz`;
  const file = path.join(directory, expected);
  ensureRegularFile(file, `${packageName} npm package`);
  return file;
}

function verifyNpmPackages(directory, version) {
  const packageDirectory = path.join(directory, 'npm');
  ensureDirectory(packageDirectory);
  const expectedFiles = packageNames.map((name) => findNpmArchive(packageDirectory, name, version)).sort();
  const actualFiles = fs.readdirSync(packageDirectory)
    .filter((name) => name.endsWith('.tgz'))
    .map((name) => path.join(packageDirectory, name))
    .sort();
  assert.deepEqual(actualFiles, expectedFiles, 'npm package set is not exactly the five native packages plus meta-package');

  const byName = new Map();
  for (const archive of actualFiles) {
    const metadata = packageJsonFromArchive(archive);
    assert.equal(metadata.version, version, `${archive} version mismatch`);
    assert.ok(packageNames.includes(metadata.name), `${archive} contains unexpected package ${metadata.name}`);
    byName.set(metadata.name, { archive, metadata, entries: packageArchiveEntries(archive) });
  }
  assert.equal(byName.size, packageNames.length, 'npm package archives contain duplicate package identities');
  for (const target of targetNames) {
    const descriptor = targets[target];
    const item = byName.get(descriptor.packageName);
    assert.ok(item, `missing ${descriptor.packageName} archive`);
    assert.ok(item.entries.includes(`package/${descriptor.binaryPath}`), `${descriptor.packageName} archive is missing ${descriptor.binaryPath}`);
    assert.deepEqual(item.metadata.os, [descriptor.os], `${descriptor.packageName} os metadata mismatch`);
    assert.deepEqual(item.metadata.cpu, [descriptor.cpu], `${descriptor.packageName} cpu metadata mismatch`);
    if (descriptor.libc) assert.deepEqual(item.metadata.libc, [descriptor.libc], `${descriptor.packageName} libc metadata mismatch`);
    assert.equal(item.metadata.crap4tsBinary, descriptor.binaryPath, `${descriptor.packageName} binary mapping mismatch`);
    const standalone = path.join(directory, versionedArchiveName(version, target));
    const tarOptions = { maxBuffer: 256 * 1024 * 1024 };
    const standaloneBytes = execFileSync('tar', ['-xOf', standalone, `crap4ts-${target}/${descriptor.binaryName}`], tarOptions);
    const npmBytes = execFileSync('tar', ['-xOf', item.archive, `package/${descriptor.binaryPath}`], tarOptions);
    assert.equal(hashFileBuffer(npmBytes), hashFileBuffer(standaloneBytes), `${target} standalone and npm binary payload differ`);
  }
  const meta = byName.get('crap4ts');
  assert.ok(meta, 'missing crap4ts meta-package archive');
  assert.ok(meta.entries.includes('package/bin/crap4ts.js'), 'meta-package archive is missing executable launcher');
  assert.deepEqual(
    Object.keys(meta.metadata.optionalDependencies || {}).sort(),
    targetNames.map((target) => targets[target].packageName).sort(),
    'meta-package optional dependencies do not cover exactly the target map',
  );
  for (const dependency of Object.values(meta.metadata.optionalDependencies || {})) {
    assert.equal(dependency, version, 'meta-package optional dependency version mismatch');
  }
}

function hashFileBuffer(value) { return crypto.createHash('sha256').update(value).digest('hex'); }

function verifyRelease(options) {
  const version = rustWorkspaceVersion();
  checkVersions({});
  const releaseDirectory = path.resolve(options.releaseDir);
  ensureDirectory(releaseDirectory);
  const manifest = path.join(releaseDirectory, 'SHA256SUMS');
  ensureRegularFile(manifest, 'SHA256SUMS');
  const files = distributableFiles(releaseDirectory, version);
  const actualArchives = actualStandaloneArchives(releaseDirectory);
  const expectedArchives = targetNames
    .map((target) => path.join(releaseDirectory, versionedArchiveName(version, target)))
    .sort();
  assert.deepEqual(actualArchives, expectedArchives, 'standalone archive set is missing or contains an extra target');
  for (const target of targetNames) {
    const archive = path.join(releaseDirectory, versionedArchiveName(version, target));
    ensureRegularFile(archive, `${target} archive`);
    assertArchiveHasOnlyRegularFiles(archive, expectedArchiveEntries(target));
  }
  verifyNpmPackages(releaseDirectory, version);
  const npmManifest = path.join(releaseDirectory, 'npm', 'SHA256SUMS');
  ensureRegularFile(npmManifest, 'npm SHA256SUMS');
  const expectedNpm = new Map(fs.readdirSync(path.join(releaseDirectory, 'npm')).filter((name) => name.endsWith('.tgz')).map((name) => [name, hashFile(path.join(releaseDirectory, 'npm', name))]));
  assert.deepEqual([...readManifest(npmManifest).keys()].sort(), [...expectedNpm.keys()].sort(), 'npm SHA256SUMS does not cover exactly the npm tarballs');
  for (const [name, digest] of expectedNpm) assert.equal(readManifest(npmManifest).get(name), digest, `npm checksum mismatch for ${name}`);

  const allowedRoot = new Set([...expectedArchives.map((file) => path.basename(file)), 'SHA256SUMS', 'npm', '.smoke.ok']);
  for (const entry of fs.readdirSync(releaseDirectory)) {
    if (!allowedRoot.has(entry)) throw new Error(`unexpected release file ${entry}`);
  }
  const allowedNpm = new Set([...fs.readdirSync(path.join(releaseDirectory, 'npm')).filter((name) => name.endsWith('.tgz')), 'SHA256SUMS']);
  for (const entry of fs.readdirSync(path.join(releaseDirectory, 'npm'))) {
    if (!allowedNpm.has(entry)) throw new Error(`unexpected npm release file ${entry}`);
  }

  const expectedManifest = new Map();
  for (const file of files) {
    expectedManifest.set(path.relative(releaseDirectory, file).replaceAll('\\', '/'), hashFile(file));
  }
  const actualManifest = readManifest(manifest);
  assert.deepEqual([...actualManifest.keys()].sort(), [...expectedManifest.keys()].sort(), 'SHA256SUMS does not cover exactly the release assets');
  for (const [name, digest] of expectedManifest) {
    assert.equal(actualManifest.get(name), digest, `checksum mismatch for ${name}`);
  }
  if (options.requireSmoke) ensureRegularFile(path.join(releaseDirectory, '.smoke.ok'), 'release smoke marker');
  process.stdout.write(`verified ${version}: ${files.length} standalone assets, five targets, npm packages, and checksums\n`);
}

function assembleRelease(options) {
  const version = rustWorkspaceVersion();
  checkVersions({});
  const binaryDirectory = path.resolve(options.binaryDir);
  const releaseDirectory = path.resolve(options.outputDir);
  clearGeneratedFiles(releaseDirectory, version);
  const stagingRoot = fs.mkdtempSync(path.join(releaseDirectory, '.standalone-'));
  const packageOutput = path.join(releaseDirectory, 'npm');
  try {
    for (const target of targetNames) {
      const source = binaryForTarget(binaryDirectory, target);
      const descriptor = targets[target];
      const folder = path.join(stagingRoot, `crap4ts-${target}`);
      fs.mkdirSync(folder, { recursive: true });
      copyFile(source, path.join(folder, descriptor.binaryName), descriptor.os !== 'win32');
      createArchive(
        stagingRoot,
        target,
        path.join(releaseDirectory, versionedArchiveName(version, target)),
      );
    }
    const sources = Object.fromEntries(targetNames.map((target) => [target, binaryForTarget(binaryDirectory, target)]));
    packTargets(sources, packageOutput, targetNames);
    const files = distributableFiles(releaseDirectory, version);
    verifyNpmPackages(releaseDirectory, version);
    writeManifest(files, path.join(releaseDirectory, 'SHA256SUMS'));
    writeManifest(fs.readdirSync(packageOutput).filter((name) => name.endsWith('.tgz')).map((name) => path.join(packageOutput, name)), path.join(packageOutput, 'SHA256SUMS'));
    // Remove staging before the strict recursive release check.
    fs.rmSync(stagingRoot, { recursive: true, force: true });
    // Verify before returning so assembly cannot hand a caller a partial set.
    verifyRelease({ releaseDir: releaseDirectory, requireSmoke: false });
    process.stdout.write(`assembled ${version} release in ${releaseDirectory}\n`);
  } finally {
    fs.rmSync(stagingRoot, { recursive: true, force: true });
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  if (options.command === 'assemble') assembleRelease(options);
  else verifyRelease(options);
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`release verification failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  archiveEntries,
  assembleRelease,
  expectedArchiveEntries,
  hashFile,
  main,
  parseArgs,
  verifyNpmPackages,
  verifyRelease,
  versionedArchiveName,
};
