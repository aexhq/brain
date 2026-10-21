import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

export async function startProcess(command, args, env) {
  const child = spawn(command, args, { env, windowsHide: true, stdio: ['pipe', 'pipe', 'inherit'] });
  const exited = new Promise(resolve => {
    child.once('error', error => resolve({ code: null, signal: null, error }));
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
  child.stdin.on('error', error => { if (error.code !== 'EPIPE') child.kill('SIGKILL'); });
  let stopping;
  const stop = () => stopping ??= (async () => {
    child.stdin.end();
    let timer;
    try {
      const result = await Promise.race([exited, new Promise(resolve => {
        timer = setTimeout(() => { child.kill('SIGKILL'); resolve({ error: new Error('Brain child shutdown timed out') }); }, 25000);
      })]);
      if (result.error) { await exited; throw result.error; }
      return result;
    } finally { clearTimeout(timer); }
  })();
  const lines = createInterface({ input: child.stdout });
  let timer;
  try {
    const ready = await Promise.race([
      new Promise((resolve, reject) => {
        lines.once('line', line => { try { resolve(JSON.parse(line)); } catch (error) { reject(error); } });
        timer = setTimeout(() => reject(new Error('Brain child startup timed out')), 30000);
      }),
      exited.then(result => { throw result.error ?? new Error(`Brain child exited before startup (${result.code ?? result.signal})`); }),
    ]);
    return { ready, exited, stop };
  } catch (error) {
    try { await stop(); } catch (cleanup) { throw new AggregateError([error, cleanup], 'Brain child startup and cleanup failed'); }
    throw error;
  } finally { clearTimeout(timer); lines.close(); }
}

export async function stopProcess(process) {
  const result = await process.stop();
  if (result.code !== 0) throw new Error(`Brain child exited with ${result.code ?? result.signal}`);
}
