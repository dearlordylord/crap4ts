#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { targets } = require('./stage-platform.js');

const root = path.resolve(__dirname, '..');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const target = `${process.platform}-${process.arch}`;

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} ${args.join(' ')} failed: ${result.error?.message || result.stderr}`);
  }
  return result;
}

function main() {
  if (!targets[target]) {
    throw new Error(`host ${target} is not a supported crap4ts npm target`);
  }
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-npm-smoke-'));
  try {
    run('cargo', ['build', '--release', '-p', 'crap4ts'], root);
    const binary = path.join(root, 'target', 'release', targets[target].binaryName || 'crap4ts');
    const outputDir = path.join(temporaryRoot, 'packages');
    run(process.execPath, [path.join(root, 'scripts', 'pack-platform.js'), '--target', target, '--binary', binary, '--output-dir', outputDir], root);

    const archives = fs.readdirSync(outputDir).filter((file) => file.endsWith('.tgz'));
    const nativeArchive = archives.find((file) => file.includes(target));
    const metaArchive = archives.find((file) => !file.includes(target));
    assert.ok(nativeArchive, `native archive for ${target} missing`);
    assert.ok(metaArchive, 'meta-package archive missing');

    const consumer = path.join(temporaryRoot, 'consumer');
    fs.mkdirSync(consumer, { recursive: true });
    const nativePackage = targets[target].packageName;
    fs.writeFileSync(path.join(consumer, 'package.json'), JSON.stringify({
      name: 'crap4ts-npm-smoke-consumer',
      private: true,
      dependencies: {
        crap4ts: `file:${path.join(outputDir, metaArchive)}`,
        [nativePackage]: `file:${path.join(outputDir, nativeArchive)}`,
      },
    }, null, 2));
    run(npm, ['install', '--ignore-scripts', '--offline', '--omit=optional', '--no-audit', '--no-fund'], consumer);

    const executable = path.join(consumer, 'node_modules', '.bin', 'crap4ts');
    const help = run(executable, ['--help'], consumer);
    assert.match(help.stdout, /Usage: crap4ts/);

    const fixtureRoot = path.join(consumer, 'fixture');
    const source = path.join(fixtureRoot, 'src', 'fixture.ts');
    fs.mkdirSync(path.dirname(source), { recursive: true });
    fs.writeFileSync(source, 'function greet(name: string) { return name; }\n');
    fs.writeFileSync(path.join(fixtureRoot, 'coverage-final.json'), JSON.stringify({
      [source]: {
        path: source,
        statementMap: { 0: { start: { line: 1, column: 31 }, end: { line: 1, column: 43 } } },
        fnMap: { 0: { name: 'greet', decl: { start: { line: 1, column: 0 }, end: { line: 1, column: 45 } }, loc: { start: { line: 1, column: 0 }, end: { line: 1, column: 45 } } } },
        s: { 0: 1 },
        f: { 0: 1 },
      },
    }));
    const analysis = run(executable, [
      '--coverage', path.join(fixtureRoot, 'coverage-final.json'),
      path.join(fixtureRoot, 'src', 'fixture.ts'), '--project-root', fixtureRoot,
      '--format', 'json', '--threshold', '1',
    ], consumer);
    const report = JSON.parse(analysis.stdout);
    assert.equal(report.rows[0].name, 'greet');
    assert.equal(report.rows[0].crap, 1);
  } finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

try {
  main();
} catch (error) {
  process.stderr.write(`npm archive smoke failed: ${error.message}\n`);
  process.exitCode = 1;
}
