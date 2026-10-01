'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');
const { pack, packTargets, removeStaged, verifyNativePack } = require('../../../scripts/pack-platform.js');
const { stage, targets } = require('../../../scripts/stage-platform.js');
const { npmInvocation } = require('../../../scripts/npm-command.js');
const { platformDescriptor, supportedPlatformKeys } = require('../bin/crap4ts.js');

const repositoryRoot = path.resolve(__dirname, '../../..');

function runNpmExecutable(args, cwd = repositoryRoot) {
  // Each test is hermetic: packaging tests may remove their temporary staged
  // payload while node:test executes sibling tests concurrently.
  const hostTarget = `${process.platform}-${process.arch}`;
  const hostBinary = path.join(repositoryRoot, 'target', 'release', targets[hostTarget].binaryName);
  if (fs.existsSync(hostBinary)) stage(hostTarget, hostBinary);
  const npm = npmInvocation();
  return spawnSync(
    npm.command,
    [
      ...npm.argsPrefix,
      'exec',
      '--silent',
      '--offline',
      '--workspace',
      'packages/crap4ts',
      '--',
      'crap4ts',
      ...args,
    ],
    { cwd, encoding: 'utf8' },
  );
}

function fixture() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-npm-')));
  const sourcePath = path.join(root, 'src', 'fixture.ts');
  fs.mkdirSync(path.dirname(sourcePath), { recursive: true });
  fs.writeFileSync(sourcePath, 'function greet(name: string) { return name; }\n');
  const coverage = {
    [sourcePath]: {
      path: sourcePath,
      statementMap: {
        0: {
          start: { line: 1, column: 31 },
          end: { line: 1, column: 43 },
        },
      },
      fnMap: {
        0: {
          name: 'greet',
          decl: {
            start: { line: 1, column: 0 },
            end: { line: 1, column: 45 },
          },
          loc: {
            start: { line: 1, column: 0 },
            end: { line: 1, column: 45 },
          },
        },
      },
      s: { 0: 1 },
      f: { 0: 1 },
    },
  };
  fs.writeFileSync(path.join(root, 'coverage-final.json'), JSON.stringify(coverage));
  return {
    root,
    cleanup() {
      fs.rmSync(root, { recursive: true, force: true });
    },
  };
}

test('npm executable forwards help output and exits successfully', () => {
  const result = runNpmExecutable(['--help']);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Usage: crap4ts/);
  assert.match(result.stdout, /--coverage/);
});

test('launcher selects bundled binaries for all five platforms', () => {
  assert.deepEqual(supportedPlatformKeys().sort(), Object.keys(targets).sort());
  for (const [key, target] of Object.entries(targets)) {
    assert.deepEqual(platformDescriptor(key), {
      binaryPath: target.binaryPath,
    });
  }
  assert.equal(platformDescriptor('freebsd-x64'), undefined);
});

test('npm executable runs the minimal fixture and forwards gate status', () => {
  const project = fixture();
  try {
    const result = runNpmExecutable([
      '--coverage',
      path.join(project.root, 'coverage-final.json'),
      path.join(project.root, 'src/fixture.ts'),
      '--project-root',
      project.root,
      '--format',
      'json',
      '--threshold',
      '1',
    ]);
    assert.equal(result.status, 0, result.stderr);
    const report = JSON.parse(result.stdout);
    assert.equal(report.version, 1);
    assert.equal(report.rows[0].path, 'src/fixture.ts');
    assert.equal(report.rows[0].name, 'greet');
    assert.equal(report.rows[0].crap, 1);
    assert.equal(result.stderr, '');

    const breach = runNpmExecutable([
      '--coverage',
      path.join(project.root, 'coverage-final.json'),
      path.join(project.root, 'src/fixture.ts'),
      '--project-root',
      project.root,
      '--format',
      'json',
      '--threshold',
      '0',
    ]);
    assert.equal(breach.status, 2, breach.stderr);
    assert.match(breach.stderr, /quality gate breached/);
    assert.equal(JSON.parse(breach.stdout).rows[0].crap, 1);
  } finally {
    project.cleanup();
  }
});

test('Rust and npm package versions are checked together', () => {
  const result = spawnSync(process.execPath, [path.join(repositoryRoot, 'scripts/check-versions.js')], {
    cwd: repositoryRoot,
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  const { version } = require('../package.json');
  assert.equal(result.stdout.trim(), `crap4ts versions aligned at ${version} (JSON schemas v1/v2)`);
});

test('Windows package accepts a mode-0644 executable payload', () => {
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-win-pack-'));
  const destination = path.join(
    repositoryRoot,
    'packages',
    'crap4ts',
    targets['win32-x64'].binaryPath,
  );
  try {
    const input = path.join(temporaryRoot, 'crap4ts.exe');
    fs.writeFileSync(input, 'windows executable fixture');
    fs.chmodSync(input, 0o644);
    stage('win32-x64', input);
    const packed = pack('crap4ts', temporaryRoot);
    assert.doesNotThrow(() => verifyNativePack(packed, 'win32-x64'));
    const payload = packed.metadata.files.find((entry) => entry.path === targets['win32-x64'].binaryPath);
    assert.ok(payload);
    assert.equal(payload.mode & 0o111, 0);
  } finally {
    fs.rmSync(destination, { force: true });
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
});

// Releases are assembled on Linux, where POSIX executable modes can be verified.
test('bundled tarball includes exactly all five distinct binary payloads', { skip: process.platform === 'win32' }, () => {
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-bundle-'));
  try {
    const sources = Object.fromEntries(Object.keys(targets).map((key) => {
      const input = path.join(temporaryRoot, key);
      fs.writeFileSync(input, `distinct binary payload for ${key}`);
      fs.chmodSync(input, key.startsWith('win32-') ? 0o644 : 0o755);
      return [key, input];
    }));
    const output = path.join(temporaryRoot, 'packed');
    const { meta } = packTargets(sources, output);
    assert.deepEqual(fs.readdirSync(output), [path.basename(meta.archive)]);
    assert.deepEqual(meta.metadata.files.map(({ path }) => path).sort(), [
      'README.md', 'package.json', 'bin/crap4ts.js',
      ...Object.values(targets).map(({ binaryPath }) => binaryPath),
    ].sort());
    for (const key of Object.keys(targets)) verifyNativePack(meta, key, sources[key]);
    const version = require('../package.json').version;
    const releaseDir = path.join(temporaryRoot, 'release');
    fs.mkdirSync(path.join(releaseDir, 'npm'), { recursive: true });
    fs.copyFileSync(meta.archive, path.join(releaseDir, 'npm', path.basename(meta.archive)));
    for (const [key, descriptor] of Object.entries(targets)) {
      const folder = `crap4ts-${key}`;
      fs.mkdirSync(path.join(temporaryRoot, folder));
      fs.copyFileSync(sources[key], path.join(temporaryRoot, folder, descriptor.binaryName));
      const tar = spawnSync('tar', ['-czf', path.join(releaseDir, `crap4ts-${version}-${key}.tar.gz`), '-C', temporaryRoot, folder]);
      assert.equal(tar.status, 0, tar.stderr?.toString());
    }
    const { verifyNpmPackages } = require('../../../scripts/release.js');
    assert.doesNotThrow(() => verifyNpmPackages(releaseDir, version));
    fs.writeFileSync(path.join(temporaryRoot, 'crap4ts-linux-arm64/crap4ts'), 'corrupted payload');
    const tar = spawnSync('tar', ['-czf', path.join(releaseDir, `crap4ts-${version}-linux-arm64.tar.gz`), '-C', temporaryRoot, 'crap4ts-linux-arm64']);
    assert.equal(tar.status, 0);
    assert.throws(() => verifyNpmPackages(releaseDir, version), /linux-arm64 standalone and bundled npm binary payload differ/);
    assert.throws(() => packTargets({ ...sources, 'linux-arm64': undefined }, output), /missing source binary for linux-arm64/);
    for (const key of Object.keys(targets)) {
      assert.equal(fs.existsSync(path.join(repositoryRoot, 'packages/crap4ts', targets[key].binaryPath)), false);
    }
  } finally {
    for (const key of Object.keys(targets)) removeStaged(key);
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
});
