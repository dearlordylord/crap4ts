'use strict';

function npmInvocation(platform = process.platform, execPath = process.execPath, npmExecPath = process.env.npm_execpath) {
  if (platform !== 'win32') return { command: 'npm', argsPrefix: [] };
  if (!npmExecPath) {
    throw new Error('npm_execpath is required to invoke npm safely on Windows');
  }
  return { command: execPath, argsPrefix: [npmExecPath] };
}

function npmBinInvocation(
  executable,
  launcher,
  args,
  platform = process.platform,
  execPath = process.execPath,
) {
  if (platform === 'win32') {
    return { command: execPath, args: [launcher, ...args], spawnOptions: {} };
  }
  return { command: executable, args, spawnOptions: {} };
}

module.exports = { npmBinInvocation, npmInvocation };
