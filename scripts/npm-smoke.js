#!/usr/bin/env node
'use strict';

// Exercise both distribution paths against the checked-in mixed workspace.
// The fixture intentionally combines an Istanbul package and an LCOV package;
// this catches wrapper/package mistakes that a one-file smoke cannot see.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { stage, targets } = require('./stage-platform.js');
const { validateReport } = require('./validate-report.js');

const root = path.resolve(__dirname, '..');
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const target = `${process.platform}-${process.arch}`;

function parseArgs(args) {
  const options = { binary: undefined, marker: undefined };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--binary' || argument === '--marker') {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) throw new Error(`${argument} requires a value`);
      options[argument.slice(2)] = value;
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      process.stdout.write('Usage: node scripts/npm-smoke.js [--binary PATH] [--marker PATH]\n');
      return null;
    } else {
      throw new Error(`unknown argument ${argument}`);
    }
  }
  return options;
}

function run(command, args, cwd, options = {}) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    ...options,
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} ${args.join(' ')} failed: ${result.error?.message || result.stderr || `exit ${result.status}`}`,
    );
  }
  return result;
}

function copyMixedFixture(destination) {
  const source = path.join(root, 'crates', 'crap4ts-cli', 'tests', 'fixtures', 'issue-nine');
  fs.cpSync(source, destination, { recursive: true });
  return destination;
}

function assertMixedReport(result, label) {
  assert.equal(result.stderr, '', `${label} wrote unexpected stderr: ${result.stderr}`);
  const report = JSON.parse(result.stdout);
  validateReport(report, 2);
  assert.equal(report.rows.length, 2, `${label} did not analyze both package groups`);
  assert.deepEqual(
    report.groups.map((group) => group.name),
    ['istanbul', 'lcov'],
    `${label} group order changed`,
  );
  assert.ok(report.rows.every((row) => row.coverage.status === 'measured'));
  return report;
}

function packageSmoke(temporaryRoot, outputDir, expectedDirect) {
  const archives = fs.readdirSync(outputDir).filter((file) => file.endsWith('.tgz'));
  const nativeArchive = archives.find((file) => file.includes(targets[target].packageName.split('/').pop()));
  const metaArchive = archives.find((file) => file.startsWith('crap4ts-') && !file.includes(target));
  assert.ok(nativeArchive, `native archive for ${target} missing`);
  assert.ok(metaArchive, 'meta-package archive missing');

  const consumer = path.join(temporaryRoot, 'consumer');
  fs.mkdirSync(consumer, { recursive: true });
  const nativePackage = targets[target].packageName;
  fs.writeFileSync(
    path.join(consumer, 'package.json'),
    JSON.stringify(
      {
        name: 'crap4ts-npm-smoke-consumer',
        private: true,
        dependencies: {
          crap4ts: `file:${path.join(outputDir, metaArchive)}`,
          [nativePackage]: `file:${path.join(outputDir, nativeArchive)}`,
        },
      },
      null,
      2,
    ),
  );
  run(
    npm,
    ['install', '--ignore-scripts', '--offline', '--omit=optional', '--no-audit', '--no-fund'],
    consumer,
  );

  const executableName = process.platform === 'win32' ? 'crap4ts.cmd' : 'crap4ts';
  const executable = path.join(consumer, 'node_modules', '.bin', executableName);
  const help = run(executable, ['--help'], consumer);
  assert.match(help.stdout, /Usage: crap4ts/);

  const fixtureRoot = copyMixedFixture(path.join(consumer, 'fixture'));
  const installed = run(executable, ['--format', 'json'], fixtureRoot);
  const report = assertMixedReport(installed, 'npm-installed binary');
  assert.equal(installed.stdout, expectedDirect.stdout, 'direct and npm-installed reports differ');
  return report;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  if (!targets[target]) {
    throw new Error(`host ${target} is not a supported crap4ts npm target`);
  }
  const temporaryRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-npm-smoke-'));
  try {
    if (!options.binary) run('cargo', ['build', '--locked', '--release', '-p', 'crap4ts'], root);
    const binary = options.binary
      ? path.resolve(options.binary)
      : path.join(root, 'target', 'release', targets[target].binaryName || 'crap4ts');
    if (!fs.existsSync(binary)) throw new Error(`smoke binary does not exist: ${binary}`);
    const directFixture = copyMixedFixture(path.join(temporaryRoot, 'direct-fixture'));
    const direct = run(binary, ['--format', 'json'], directFixture);
    assertMixedReport(direct, 'direct binary');

    const outputDir = path.join(temporaryRoot, 'packages');
    run(
      process.execPath,
      [
        path.join(root, 'scripts', 'pack-platform.js'),
        '--target',
        target,
        '--binary',
        binary,
        '--output-dir',
        outputDir,
      ],
      root,
    );
    packageSmoke(temporaryRoot, outputDir, direct);
    // The workspace package tests run immediately after this pretest hook and
    // exercise the launcher through npm's workspace symlink. Keep the host
    // payload available for that local-only test; release assembly uses
    // packAll directly and always removes staged files in its finally block.
    stage(target, binary);
    if (options.marker) {
      fs.mkdirSync(path.dirname(path.resolve(options.marker)), { recursive: true });
      fs.writeFileSync(path.resolve(options.marker), 'mixed Istanbul/LCOV direct and npm smoke passed\n');
    }
    process.stdout.write(`npm smoke passed for ${target} (mixed Istanbul/LCOV workspace)\n`);
  } finally {
    fs.rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`npm archive smoke failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = {
  assertMixedReport,
  copyMixedFixture,
  main,
  parseArgs,
};
