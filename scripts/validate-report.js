#!/usr/bin/env node
'use strict';

// The checked-in Draft 2020-12 schemas are the sole report contract.
const fs = require('node:fs');
const path = require('node:path');
const Ajv2020 = require('ajv/dist/2020');

const root = path.resolve(__dirname, '..');
const ajv = new Ajv2020({ allErrors: true, strict: true });
const validators = new Map([1, 2].map((version) => {
  const schema = JSON.parse(fs.readFileSync(path.join(root, 'schemas', `report-v${version}.schema.json`), 'utf8'));
  return [version, ajv.compile(schema)];
}));

function validateReport(report, version = report?.version) {
  const validate = validators.get(version);
  if (!validate) throw new Error(`unsupported report schema version ${version}`);
  if (!validate(report)) {
    throw new Error(`report v${version} does not match its JSON Schema: ${ajv.errorsText(validate.errors, { separator: '; ' })}`);
  }
  return true;
}

module.exports = { validateReport, validators };
