#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { spawnSync } = require('node:child_process');
const { validateReport } = require('./validate-report.js');

const root = path.resolve(__dirname, '..');

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function sample(version) {
  const row = {
    id: version % 2 === 0 ? 'group::src/file.ts::fn' : 'src/file.ts::fn',
    ...(version % 2 === 0 ? { group: 'group' } : {}),
    path: version % 2 === 0 ? 'packages/group/src/file.ts' : 'src/file.ts',
    name: 'fn',
    kind: 'function_declaration',
    range: {
      start: { offset: 0, line: 1, column: 0 },
      end: { offset: 10, line: 1, column: 10 },
    },
    complexity: 1,
    coverage: { status: 'measured', covered: 1, total: 1, fraction: 1 },
    crap: 1,
  };
  return version % 2 === 0
    ? {
        version,
        rows: [row],
        diagnostics: [],
        groups: [{ name: 'group', root: 'packages/group', threshold: 8, report_only: false }],
      }
    : { version, threshold: 8, rows: [row], diagnostics: [] };
}

function validateSchemaDocuments() {
  for (const version of [1, 2, 3, 4]) {
    const file = path.join(root, 'schemas', `report-v${version}.schema.json`);
    const schema = readJson(file);
    assert.equal(schema.type, 'object', `${file} must describe an object`);
    assert.equal(schema.properties.version.const, version, `${file} version mismatch`);
    const document = sample(version);
    if (version >= 3) {
      document.rows[0].label = 'GET /users';
      document.rows[0].coverage.basis = 'branch';
    }
    validateReport(document, version);
    if (version < 3) {
      document.rows[0].coverage.basis = 'branch';
      assert.throws(() => validateReport(document, version));
    }
  }
}

function validateBinary(binary) {
  const fixture = path.join(root, 'crates', 'crap4ts-cli', 'tests', 'fixtures', 'issue-nine');
  const executable = path.resolve(binary);
  const single = spawnSync(executable, ['--coverage', 'coverage-final.json', 'src', '--format', 'json'], {
    cwd: path.join(fixture, 'packages', 'istanbul'),
    encoding: 'utf8',
  });
  if (single.error || single.status !== 0) {
    throw new Error(`v1 schema smoke failed: ${single.error?.message || single.stderr}`);
  }
  validateReport(JSON.parse(single.stdout), 1);
  const result = spawnSync(executable, ['--format', 'json'], {
    cwd: fixture,
    encoding: 'utf8',
  });
  if (result.error || result.status !== 0) {
    throw new Error(`schema smoke failed: ${result.error?.message || result.stderr}`);
  }
  validateReport(JSON.parse(result.stdout), 2);
  const branched = spawnSync(executable, ['--coverage', 'coverage-final.json', 'src', '--format', 'json', '--coverage-metric', 'branch'], {
    cwd: path.join(fixture, 'packages', 'istanbul'), encoding: 'utf8',
  });
  assert.equal(branched.status, 0, branched.stderr);
  validateReport(JSON.parse(branched.stdout), 3);

  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'crap4ts-schemas-'));
  try {
    fs.cpSync(fixture, temporary, { recursive: true });
    const configPath = path.join(temporary, 'crap4ts.json');
    const config = readJson(configPath);
    for (const group of Object.values(config.groups)) group.coverage_metric = 'branch';
    fs.writeFileSync(configPath, JSON.stringify(config));
    const aggregate = spawnSync(executable, ['--json'], { cwd: temporary, encoding: 'utf8' });
    assert.equal(aggregate.status, 0, aggregate.stderr);
    validateReport(JSON.parse(aggregate.stdout), 4);
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }

}

function main() {
  validateSchemaDocuments();
  const binaryIndex = process.argv.indexOf('--binary');
  if (binaryIndex >= 0) {
    const binary = process.argv[binaryIndex + 1];
    if (!binary) throw new Error('--binary requires a path');
    validateBinary(binary);
  }
  process.stdout.write('validated JSON report schemas v1, v2, v3, and v4\n');
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`schema validation failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { main, validateSchemaDocuments };
