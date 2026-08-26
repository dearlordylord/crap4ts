#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const https = require('node:https');

function validatedUrl(value) {
  const url = new URL(value);
  if (url.protocol !== 'https:') throw new Error('download URL must use HTTPS');
  return url;
}

function download(urlValue, destination, redirects = 5) {
  const url = validatedUrl(urlValue);
  return new Promise((resolve, reject) => {
    const request = https.get(url, (response) => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        response.resume();
        if (redirects === 0) return reject(new Error('too many download redirects'));
        const redirected = new URL(response.headers.location, url).toString();
        return download(redirected, destination, redirects - 1).then(resolve, reject);
      }
      if (response.statusCode !== 200) {
        response.resume();
        return reject(new Error(`download returned HTTP ${response.statusCode}`));
      }
      const output = fs.createWriteStream(destination, { flags: 'wx', mode: 0o600 });
      response.pipe(output);
      response.once('aborted', () => output.destroy(new Error('download response was truncated')));
      output.once('finish', () => output.close(resolve));
      output.once('error', reject);
    });
    request.once('error', reject);
  }).catch((error) => {
    fs.rmSync(destination, { force: true });
    throw error;
  });
}

async function main(args = process.argv.slice(2)) {
  if (args.length !== 2) throw new Error('Usage: node scripts/download.js URL DESTINATION');
  await download(args[0], args[1]);
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`download failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}

module.exports = { download, main, validatedUrl };
