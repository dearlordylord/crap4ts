#!/usr/bin/env node
'use strict';

// Local credentials control publication; GitHub Actions only builds and
// assembles cross-platform artifacts. Remote writes are resumable and never
// replace different immutable bytes.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const targets = require('../release-targets.json');

const root = path.resolve(__dirname, '..');
const META_PACKAGE_NAME = '@crap4ts/crap4ts';

function packageTarballName(name, version) {
  return `${name.replace(/^@/, '').replace('/', '-')}-${version}.tgz`;
}

function npmPublicationPlan(version, releaseDirectory = 'dist/release') {
  const names = Object.values(targets).map(({ packageName }) => packageName).sort();
  names.push(META_PACKAGE_NAME);
  return names.map((name) => ({
    name,
    tarball: packageTarballName(name, version),
    file: path.join(releaseDirectory, 'npm', packageTarballName(name, version)),
  }));
}

function command(name, args, options = {}) {
  const result = spawnSync(name, args, {
    cwd: root,
    encoding: options.inherit ? undefined : 'utf8',
    stdio: options.inherit ? 'inherit' : 'pipe',
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    const detail = result.error?.message || result.stderr?.trim() || `exit ${result.status}`;
    const error = new Error(`${name} ${args.join(' ')} failed: ${detail}`);
    error.status = result.status;
    error.stderr = result.stderr || '';
    throw error;
  }
  return (result.stdout || '').trim();
}

function attempt(name, args) {
  return spawnSync(name, args, { cwd: root, encoding: 'utf8' });
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function assertCleanPushedMaster() {
  assert.equal(command('git', ['branch', '--show-current']), 'master', 'local-release must run on master');
  assert.equal(command('git', ['status', '--porcelain']), '', 'local-release requires a clean worktree');
  command('git', ['fetch', 'origin', 'master']);
  const head = command('git', ['rev-parse', 'HEAD']);
  assert.equal(head, command('git', ['rev-parse', 'origin/master']), 'master must be pushed and synchronized with origin/master');
  return head;
}

function assertGreenCi(head) {
  const raw = command('gh', ['run', 'list', '--workflow', 'CI', '--branch', 'master', '--limit', '20', '--json', 'databaseId,headSha,status,conclusion']);
  const runs = JSON.parse(raw);
  assert.ok(selectSuccessfulCi(runs, head), `CI has no successful completed run for ${head}`);
}

function selectSuccessfulCi(runs, head) {
  return runs.find((run) => run.headSha === head && run.status === 'completed' && run.conclusion === 'success');
}

function ensureTag(tag, head) {
  const remote = attempt('git', ['ls-remote', '--exit-code', '--tags', 'origin', `refs/tags/${tag}`]);
  if (remote.status === 0) {
    command('git', ['fetch', 'origin', `refs/tags/${tag}:refs/tags/${tag}`]);
    assert.equal(command('git', ['rev-list', '-n', '1', tag]), head, `${tag} already points at another commit`);
    return;
  }
  if (remote.status !== 2) throw new Error(`could not inspect remote tag ${tag}: ${remote.stderr.trim()}`);
  command('git', ['tag', '-a', tag, '-m', `crap4ts ${tag}`]);
  command('git', ['push', 'origin', `refs/tags/${tag}`], { inherit: true });
}

function findReleaseRun(head) {
  for (let index = 0; index < 40; index += 1) {
    const runs = JSON.parse(command('gh', ['run', 'list', '--workflow', 'Release', '--limit', '20', '--json', 'databaseId,event,headSha']));
    const run = selectReleaseRun(runs, head);
    if (run) return String(run.databaseId);
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 3000);
  }
  throw new Error(`Release workflow did not start for ${head}`);
}

function selectReleaseRun(runs, head) {
  return runs.find((run) => run.headSha === head && run.event === 'push');
}

function downloadArtifacts(runId, temporaryDirectory) {
  const releaseDirectory = path.join(temporaryDirectory, 'release');
  const trustedDirectory = path.join(temporaryDirectory, 'trusted');
  fs.mkdirSync(releaseDirectory);
  fs.mkdirSync(trustedDirectory);
  command('gh', ['run', 'download', runId, '--name', 'crap4ts-release', '--dir', releaseDirectory], { inherit: true });
  command('gh', ['run', 'download', runId, '--name', 'crap4ts-trusted-binary-manifest', '--dir', trustedDirectory], { inherit: true });
  command(process.execPath, [path.join(root, 'scripts', 'release-gate.js'), '--release-dir', releaseDirectory, '--binary-manifest', path.join(trustedDirectory, 'BINARY-SHA256SUMS')], { inherit: true });
  return releaseDirectory;
}

function releaseAssetFiles(releaseDirectory, version) {
  return [
    ...Object.keys(targets).sort().map((target) => path.join(releaseDirectory, `crap4ts-${version}-${target}.tar.gz`)),
    path.join(releaseDirectory, 'SHA256SUMS'),
    path.join(releaseDirectory, 'BINARY-SHA256SUMS'),
    path.join(releaseDirectory, 'npm', 'NPM-SHA256SUMS'),
  ];
}

function reconcileRelease(tag, version, releaseDirectory, temporaryDirectory) {
  const existing = attempt('gh', ['release', 'view', tag, '--json', 'isDraft,tagName,assets']);
  if (existing.status !== 0) {
    command('gh', ['release', 'create', tag, '--draft', '--verify-tag', '--title', `crap4ts ${tag}`, '--notes', 'Verified cross-platform release.'], { inherit: true });
  }
  const release = JSON.parse(command('gh', ['release', 'view', tag, '--json', 'isDraft,tagName,assets']));
  assert.equal(release.tagName, tag);
  const remoteByName = new Map(release.assets.map((asset) => [asset.name, asset]));
  const files = releaseAssetFiles(releaseDirectory, version);
  const expected = new Set(files.map((file) => path.basename(file)));
  for (const asset of release.assets) assert.ok(expected.has(asset.name), `unexpected existing release asset ${asset.name}`);
  for (const file of files) {
    assert.ok(fs.statSync(file).isFile(), `release asset is missing: ${file}`);
    const name = path.basename(file);
    const remote = remoteByName.get(name);
    if (!remote) {
      command('gh', ['release', 'upload', tag, file], { inherit: true });
    } else if (remote.digest) {
      assert.equal(remote.digest.replace(/^sha256:/, ''), sha256(file), `existing release asset ${name} has different bytes`);
    } else {
      const comparisonDirectory = path.join(temporaryDirectory, 'remote-assets');
      fs.mkdirSync(comparisonDirectory, { recursive: true });
      command('gh', ['release', 'download', tag, '--pattern', name, '--dir', comparisonDirectory]);
      assert.equal(sha256(path.join(comparisonDirectory, name)), sha256(file), `existing release asset ${name} has different bytes`);
    }
  }
  return release.isDraft;
}

function publishNpmPackages(version, releaseDirectory, temporaryDirectory) {
  for (const item of npmPublicationPlan(version, releaseDirectory)) {
    assert.ok(fs.statSync(item.file).isFile(), `npm tarball is missing: ${item.file}`);
    const viewed = attempt('npm', ['view', `${item.name}@${version}`, 'dist.tarball', '--json']);
    if (viewed.status !== 0) {
      if (!/E404|404 Not Found/.test(viewed.stderr)) throw new Error(`npm view failed for ${item.name}@${version}: ${viewed.stderr.trim()}`);
      command('npm', ['publish', item.file, '--access', 'public'], { inherit: true });
      continue;
    }
    const url = JSON.parse(viewed.stdout);
    assert.ok(typeof url === 'string' && url.startsWith('https://'), `npm returned an invalid tarball URL for ${item.name}@${version}`);
    const remoteFile = path.join(temporaryDirectory, `remote-${item.tarball}`);
    command('curl', ['--fail', '--location', '--silent', '--show-error', url, '--output', remoteFile]);
    assert.equal(sha256(remoteFile), sha256(item.file), `npm already has different bytes for ${item.name}@${version}`);
    process.stdout.write(`skip identical npm package ${item.name}@${version}\n`);
  }
}

function parseArgs(args) {
  if (args.length === 0) return { check: false };
  if (args.length === 1 && args[0] === '--check') return { check: true };
  throw new Error('Usage: pnpm local-release [--check]');
}

function main(args = process.argv.slice(2)) {
  const options = parseArgs(args);
  const version = require('../package.json').version;
  const tag = `v${version}`;
  const head = assertCleanPushedMaster();
  command('gh', ['auth', 'status']);
  command('npm', ['whoami']);
  assertGreenCi(head);
  command(process.execPath, [path.join(root, 'scripts', 'check-versions.js'), '--tag', tag]);
  if (options.check) {
    process.stdout.write(`release preflight passed for ${tag} at ${head}\n`);
    return;
  }
  ensureTag(tag, head);
  const runId = findReleaseRun(head);
  command('gh', ['run', 'watch', runId, '--exit-status'], { inherit: true });
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), `crap4ts-${tag}-`));
  try {
    const releaseDirectory = downloadArtifacts(runId, temporaryDirectory);
    const wasDraft = reconcileRelease(tag, version, releaseDirectory, temporaryDirectory);
    publishNpmPackages(version, releaseDirectory, temporaryDirectory);
    if (wasDraft) command('gh', ['release', 'edit', tag, '--draft=false', '--latest'], { inherit: true });
    process.stdout.write(`released ${META_PACKAGE_NAME}@${version} from ${head}\n`);
  } finally {
    fs.rmSync(temporaryDirectory, { recursive: true, force: true });
  }
}

if (require.main === module) {
  try { main(); } catch (error) {
    process.stderr.write(`local release failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  main,
  npmPublicationPlan,
  packageTarballName,
  parseArgs,
  releaseAssetFiles,
  selectReleaseRun,
  selectSuccessfulCi,
};
