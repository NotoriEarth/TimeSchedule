import { strict as assert } from 'node:assert';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { copyFile, mkdtemp, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';

const executable = process.env.TIMESCHEDULE_RECORD_TEST_EXE;

test('Claude stdio releases the executable while stdin stays open and follows saved power', { skip: !executable }, async t => {
  const dir = await mkdtemp(path.join(tmpdir(), 'timeschedule-mcp-power-'));
  const exe = path.join(dir, 'TimeSchedule.exe');
  const children = [];
  t.after(async () => {
    for (const { child, closed } of children) {
      if (child.exitCode === null) child.kill();
      await closed;
    }
    await rm(dir, { recursive: true, force: true });
  });
  await copyFile(executable, exe);
  const setPower = async enabled => {
    const tmp = path.join(dir, 'power.tmp');
    await writeFile(tmp, JSON.stringify({ enabled, generation: randomUUID() }));
    await rename(tmp, path.join(dir, 'mcp-power.json'));
  };
  function start(inherited) {
    const child = spawn(exe, ['--mcp-stdio', '--data-dir', dir], {
      env: { ...process.env, TIMESCHEDULE_MCP_DISABLED: inherited }, windowsHide: true,
    });
    const closed = once(child, 'close');
    children.push({ child, closed });
    return child;
  }
  async function ready(child) {
    const reply = once(child.stdout, 'data', { signal: AbortSignal.timeout(5000) });
    child.stdin.write('{"jsonrpc":"2.0","method":"ping","id":7}\n');
    assert.equal(JSON.parse((await reply)[0]).id, 7);
  }
  async function exits(child) {
    const [code] = await once(child, 'exit', { signal: AbortSignal.timeout(4000) });
    assert.equal(code, 0);
  }

  await setPower(true);
  const idle = start('1');
  await ready(idle);
  const stopped = exits(idle);
  await setPower(false);
  await stopped;
  await exits(start('0')); // Restarted Claude still has its old environment.
  await copyFile(executable, exe); // Windows denies overwriting a running image.

  await setPower(true);
  const resumed = start('1');
  await ready(resumed);
  const retired = exits(resumed);
  await setPower(false);
  await setPower(true);
  await retired; // Even a short off interval ends the old session.
  const fresh = start('1');
  await ready(fresh);
  const done = exits(fresh);
  fresh.stdin.end();
  await done;
});
