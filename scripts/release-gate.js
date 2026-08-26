#!/usr/bin/env node
'use strict';

// This is the final publication gate. It deliberately has no upload or npm
// publish behavior: a caller must prove that assembly, checksums, packages,
// and the smoke marker are complete before selecting a publish step.

const fs = require('node:fs');
const path = require('node:path');
const { verifyRelease } = require('./release.js');
const { targets } = require('./stage-platform.js');
const crypto = require('node:crypto');

const root = path.resolve(__dirname, '..');

function parseArgs(args) {
  const options = { releaseDir: path.join(root, 'dist', 'release'), requireSmoke: true, binaryManifest: undefined };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === '--release-dir') {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) throw new Error('--release-dir requires a value');
      options.releaseDir = value;
      index += 1;
    } else if (argument === '--binary-manifest') {
      const value = args[index + 1];
      if (!value || value.startsWith('-')) throw new Error('--binary-manifest requires a value');
      options.binaryManifest = value;
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      process.stdout.write('Usage: node scripts/release-gate.js [--release-dir DIR] [--binary-manifest FILE]\n');
      return null;
    } else {
      throw new Error(`unknown argument ${argument}`);
    }
  }
  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (!options) return;
  const marker = path.join(path.resolve(options.releaseDir), '.smoke.ok');
  if (!fs.existsSync(marker)) {
    throw new Error(`release smoke marker is missing: ${marker}`);
  }
  let smoke;
  try { smoke = JSON.parse(fs.readFileSync(marker, 'utf8')); } catch (error) { throw new Error(`release smoke marker is not valid JSON: ${error.message}`); }
  if (smoke.version !== require('../package.json').version || !targets[smoke.target] || !smoke.packages || !smoke.directReportSha256) throw new Error('release smoke marker metadata is incomplete or unsupported');
  const reportFile = path.join(path.resolve(options.releaseDir), '.smoke-report.json');
  if (!fs.existsSync(reportFile)) throw new Error('release smoke report is missing');
  const reportDigest = crypto.createHash('sha256').update(fs.readFileSync(reportFile)).digest('hex');
  if (reportDigest !== smoke.directReportSha256) throw new Error('release smoke report digest mismatch');
  const version = require('../package.json').version;
  const expectedNames = new Set(['crap4ts-' + version + '.tgz', ...Object.values(targets).map((d) => `${d.packageName.replace(/^@/, '').replace('/', '-')}-${version}.tgz`)]);
  if (Object.keys(smoke.packages).length !== expectedNames.size || Object.keys(smoke.packages).some((name) => !expectedNames.has(name))) throw new Error('release smoke marker must contain exactly six expected npm packages');
  for (const [name, expected] of Object.entries(smoke.packages)) {
    const file = path.join(path.resolve(options.releaseDir), 'npm', name);
    if (!fs.existsSync(file)) throw new Error(`smoke marker references missing package ${name}`);
    const actual = crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
    if (actual !== expected) throw new Error(`smoke marker digest mismatch for ${name}`);
  }
  verifyRelease(options);
  process.stdout.write('publication gate passed; no publication was performed\n');
}

if (require.main === module) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`publication gate failed: ${error.message}\n`);
    process.exitCode = 1;
  }
}

module.exports = { main, parseArgs };
