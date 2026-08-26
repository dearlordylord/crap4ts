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
const { npmBinInvocation, npmInvocation } = require('./npm-command.js');

const root = path.resolve(__dirname, '..');
const target = `${process.platform}-${process.arch}`;

function parseArgs(args) {
  const options = { binary: undefined, marker: undefined, releaseDir: undefined };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--binary' || argument === '--marker' || argument === '--release-dir') {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) throw new Error(`${argument} requires a value`);
      const key = argument.slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
      options[key] = value;
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      process.stdout.write('Usage: node scripts/npm-smoke.js --release-dir DIR --binary PATH [--marker PATH]\n');
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

function smokeConsumerDependencies(metaArchive, nativePackage, nativeArchive) {
  return {
    '@crap4ts/crap4ts': `file:${metaArchive}`,
    [nativePackage]: `file:${nativeArchive}`,
  };
}

function packageSmoke(temporaryRoot, outputDir, expectedDirect) {
  const archives = fs.readdirSync(outputDir).filter((file) => file.endsWith('.tgz'));
  const nativeArchive = archives.find((file) => file.includes(targets[target].packageName.split('/').pop()));
  const metaArchive = archives.find((file) => file.startsWith('crap4ts-crap4ts-'));
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
        dependencies: smokeConsumerDependencies(
          path.join(outputDir, metaArchive),
          nativePackage,
          path.join(outputDir, nativeArchive),
        ),
      },
      null,
      2,
    ),
  );
  const npm = npmInvocation();
  run(
    npm.command,
    [...npm.argsPrefix, 'install', '--ignore-scripts', '--offline', '--omit=optional', '--no-audit', '--no-fund'],
    consumer,
  );

  const executableName = process.platform === 'win32' ? 'crap4ts.cmd' : 'crap4ts';
  const executable = path.join(consumer, 'node_modules', '.bin', executableName);
  const launcher = path.join(
    consumer,
    'node_modules',
    '@crap4ts',
    'crap4ts',
    'bin',
    'crap4ts.js',
  );
  const invocation = (args) => npmBinInvocation(executable, launcher, args);
  const helpCommand = invocation(['--help']);
  const help = run(helpCommand.command, helpCommand.args, consumer, helpCommand.spawnOptions);
  assert.match(help.stdout, /Usage: crap4ts/);

  const fixtureRoot = copyMixedFixture(path.join(consumer, 'fixture'));
  const analysisCommand = invocation(['--format', 'json']);
  const installed = run(analysisCommand.command, analysisCommand.args, fixtureRoot, analysisCommand.spawnOptions);
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

    const outputDir = options.releaseDir
      ? path.join(path.resolve(options.releaseDir), 'npm')
      : path.join(temporaryRoot, 'packages');
    if (!options.releaseDir) {
      run(process.execPath, [path.join(root, 'scripts', 'pack-platform.js'), '--target', target, '--binary', binary, '--output-dir', outputDir], root);
    }
    packageSmoke(temporaryRoot, outputDir, direct);
    // The workspace package tests run immediately after this pretest hook and
    // exercise the launcher through npm's workspace symlink. Keep the host
    // payload available for that local-only test; release assembly uses
    // packAll directly and always removes staged files in its finally block.
    stage(target, binary);
    if (options.marker) {
      fs.mkdirSync(path.dirname(path.resolve(options.marker)), { recursive: true });
      const reportFile = path.join(path.dirname(path.resolve(options.marker)), '.smoke-report.json');
      fs.writeFileSync(reportFile, direct.stdout);
      const digest = (value) => require('node:crypto').createHash('sha256').update(value).digest('hex');
      const packages = fs.readdirSync(outputDir).filter((file) => file.endsWith('.tgz')).sort();
      fs.writeFileSync(path.resolve(options.marker), `${JSON.stringify({ version: require('../package.json').version, target, packages: Object.fromEntries(packages.map((file) => [file, digest(fs.readFileSync(path.join(outputDir, file)))])), directReportSha256: digest(direct.stdout) }, null, 2)}\n`);
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
  smokeConsumerDependencies,
};
