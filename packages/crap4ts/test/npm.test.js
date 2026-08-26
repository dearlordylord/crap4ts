'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

const repositoryRoot = path.resolve(__dirname, '../../..');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';

function runNpmExecutable(args, cwd = repositoryRoot) {
  return spawnSync(
    npm,
    [
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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-npm-'));
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
  assert.match(result.stdout, /versions aligned at 0\.1\.0/);
});
