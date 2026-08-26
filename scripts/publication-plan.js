#!/usr/bin/env node
'use strict';

// Pure, testable classification used by the publication job. Remote metadata
// is supplied by the caller (GitHub/npm APIs); this module never publishes.
const fs = require('node:fs');
const crypto = require('node:crypto');

function digest(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function classify(localFiles, remoteDigests) {
  return localFiles.map((file) => {
    const name = file.name || file.path;
    const local = file.sha256 || digest(file.path);
    const remote = remoteDigests[name];
    if (!remote) return { name, sha256: local, action: 'publish' };
    if (remote === local) return { name, sha256: local, action: 'skip-identical' };
    return { name, sha256: local, action: 'conflict' };
  });
}

function assertSafe(plan) {
  const conflict = plan.find((item) => item.action === 'conflict');
  if (conflict) throw new Error(`remote publication conflicts with ${conflict.name}`);
  return plan;
}

module.exports = { assertSafe, classify, digest };
