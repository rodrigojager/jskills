import {execFile} from 'node:child_process';

// No shell, bounded capture and lifetime, including on Windows.
export function execute(file, args, {signal, timeout = 8000, maxBuffer = 2 * 1024 * 1024, cwd} = {}) {
  return new Promise((resolve, reject) => {
    execFile(file, args, {cwd, signal, timeout, maxBuffer, windowsHide:true, encoding:'utf8'}, (error, stdout, stderr) => {
      if (error && (typeof error.code !== 'number' || error.killed)) return reject(error);
      resolve({code:error?.code ?? 0, stdout, stderr});
    });
  });
}
