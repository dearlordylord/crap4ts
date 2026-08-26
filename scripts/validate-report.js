#!/usr/bin/env node
'use strict';

// A dependency-free validator for the two public report contracts. Release
// verification uses this in addition to the checked-in JSON Schema documents,
// so a smoke run cannot accidentally assert only that stdout is parseable.

const assert = require('node:assert/strict');

const KINDS = new Set([
  'arrow',
  'constructor',
  'function_declaration',
  'function_expression',
  'getter',
  'method',
  'setter',
]);
const CATEGORIES = new Set([
  'configuration',
  'coverage_attribution',
  'coverage_command',
  'coverage_parsing',
  'missing_evidence',
  'source_parsing',
  'threshold_breach',
  'unsafe_path',
]);

function object(value, label) {
  assert.equal(typeof value, 'object', `${label} must be an object`);
  assert.notEqual(value, null, `${label} must not be null`);
  assert.equal(Array.isArray(value), false, `${label} must not be an array`);
}

function keys(value, expected, label) {
  const allowed = new Set(expected);
  for (const key of Object.keys(value)) {
    assert.ok(allowed.has(key), `${label} has unknown property ${key}`);
  }
}

function nonEmptyString(value, label) {
  assert.equal(typeof value, 'string', `${label} must be a string`);
  assert.ok(value.length > 0, `${label} must not be empty`);
}

function position(value, label) {
  object(value, label);
  keys(value, ['offset', 'line', 'column'], label);
  assertInteger(value.offset, `${label}.offset`);
  assertInteger(value.line, `${label}.line`);
  assertInteger(value.column, `${label}.column`);
  assert.ok(value.line >= 1, `${label}.line must be positive`);
  assert.ok(value.column >= 0, `${label}.column must not be negative`);
}

function range(value, label) {
  object(value, label);
  keys(value, ['start', 'end'], label);
  position(value.start, `${label}.start`);
  position(value.end, `${label}.end`);
}

function assertInteger(value, label) {
  assert.equal(typeof value, 'number', `${label} must be a number`);
  assert.ok(Number.isSafeInteger(value), `${label} must be an integer`);
}

function coverage(value, label) {
  object(value, label);
  nonEmptyString(value.status, `${label}.status`);
  if (value.status === 'measured') {
    keys(value, ['status', 'covered', 'total', 'fraction'], label);
    assertInteger(value.covered, `${label}.covered`);
    assertInteger(value.total, `${label}.total`);
    assert.ok(value.covered >= 0, `${label}.covered must not be negative`);
    assert.ok(value.total > 0, `${label}.total must be positive`);
    assert.ok(value.covered <= value.total, `${label}.covered exceeds total`);
    assert.equal(typeof value.fraction, 'number', `${label}.fraction must be a number`);
    assert.ok(Number.isFinite(value.fraction), `${label}.fraction must be finite`);
    assert.ok(value.fraction >= 0 && value.fraction <= 1, `${label}.fraction out of range`);
  } else if (value.status === 'unknown') {
    keys(value, ['status', 'reason'], label);
    nonEmptyString(value.reason, `${label}.reason`);
  } else {
    assert.fail(`${label}.status ${JSON.stringify(value.status)} is unsupported`);
  }
}

function diagnostic(value, label, aggregate) {
  object(value, label);
  keys(value, aggregate ? ['group', 'category', 'message'] : ['category', 'message'], label);
  if (value.group !== undefined) nonEmptyString(value.group, `${label}.group`);
  assert.ok(CATEGORIES.has(value.category), `${label}.category is unsupported`);
  nonEmptyString(value.message, `${label}.message`);
}

function row(value, label, aggregate) {
  object(value, label);
  keys(
    value,
    aggregate
      ? ['id', 'group', 'path', 'name', 'kind', 'range', 'complexity', 'coverage', 'crap']
      : ['id', 'path', 'name', 'kind', 'range', 'complexity', 'coverage', 'crap'],
    label,
  );
  nonEmptyString(value.id, `${label}.id`);
  if (aggregate) nonEmptyString(value.group, `${label}.group`);
  nonEmptyString(value.path, `${label}.path`);
  nonEmptyString(value.name, `${label}.name`);
  assert.ok(KINDS.has(value.kind), `${label}.kind is unsupported`);
  range(value.range, `${label}.range`);
  assertInteger(value.complexity, `${label}.complexity`);
  assert.ok(value.complexity >= 1, `${label}.complexity must be positive`);
  coverage(value.coverage, `${label}.coverage`);
  assert.ok(value.crap === null || typeof value.crap === 'number', `${label}.crap must be a number or null`);
  if (typeof value.crap === 'number') assert.ok(Number.isFinite(value.crap), `${label}.crap must be finite`);
}

function reportGroup(value, label) {
  object(value, label);
  keys(value, ['name', 'root', 'threshold', 'report_only', 'threshold_overrides'], label);
  nonEmptyString(value.name, `${label}.name`);
  nonEmptyString(value.root, `${label}.root`);
  assertInteger(value.threshold, `${label}.threshold`);
  assert.ok(value.threshold >= 0, `${label}.threshold must not be negative`);
  assert.equal(typeof value.report_only, 'boolean', `${label}.report_only must be boolean`);
  if (value.threshold_overrides !== undefined) {
    object(value.threshold_overrides, `${label}.threshold_overrides`);
    for (const [path, threshold] of Object.entries(value.threshold_overrides)) {
      nonEmptyString(path, `${label}.threshold_overrides key`);
      assertInteger(threshold, `${label}.threshold_overrides.${path}`);
      assert.ok(threshold >= 0, `${label}.threshold_overrides.${path} must not be negative`);
    }
  }
}

function validateReport(value, expectedVersion) {
  object(value, 'report');
  const aggregate = expectedVersion === 2;
  assert.equal(value.version, expectedVersion, `report must be schema version ${expectedVersion}`);
  if (aggregate) {
    keys(value, ['version', 'rows', 'diagnostics', 'groups'], 'report');
    assert.equal(value.threshold, undefined, 'v2 report must not contain a global threshold');
    assert.ok(Array.isArray(value.groups), 'v2 groups must be an array');
    value.groups.forEach((group, index) => reportGroup(group, `report.groups[${index}]`));
  } else {
    keys(value, ['version', 'threshold', 'rows', 'diagnostics'], 'report');
    assertInteger(value.threshold, 'report.threshold');
    assert.ok(value.threshold >= 0, 'report.threshold must not be negative');
  }
  assert.ok(Array.isArray(value.rows), 'report.rows must be an array');
  assert.ok(Array.isArray(value.diagnostics), 'report.diagnostics must be an array');
  value.rows.forEach((entry, index) => row(entry, `report.rows[${index}]`, aggregate));
  value.diagnostics.forEach((entry, index) => diagnostic(entry, `report.diagnostics[${index}]`, aggregate));
  return value;
}

module.exports = { validateReport };

