import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { git } from './development-diary-git.mjs';

export function runNodeTests(repo, files) {
  if (!files.length || files.some(file => !/^tests\/(?:[\w.-]+\/)*[\w.-]+\.test\.mjs$/.test(file) || file.split('/').includes('..'))) {
    throw new Error('실행할 tests/ 아래의 .test.mjs 파일을 명시하세요.');
  }
  for (const file of files) {
    if (!fs.statSync(path.join(repo, file)).isFile()) throw new Error('테스트 파일을 찾을 수 없습니다. ' + file);
  }
  const args = ['--test', '--test-reporter=tap', ...new Set(files)];
  // Node marks its test-worker children. This is an independent runner, including
  // when a regression test exercises the diary itself.
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const run = spawnSync(process.execPath, args, { cwd: repo, env, encoding: 'utf8', windowsHide: true, timeout: 180000, maxBuffer: 8 * 1024 * 1024 });
  if (run.error) throw run.error;
  const counts = {};
  for (const key of ['tests', 'pass', 'fail', 'cancelled', 'skipped', 'todo']) {
    const match = run.stdout.match(new RegExp('^# ' + key + ' (\\d+)\\r?$', 'm'));
    if (!match || run.status == null) throw new Error('Node 테스트가 완전한 TAP 집계를 반환하지 않았습니다. ' + key);
    counts[key] = Number(match[1]);
  }
  const failures = [...run.stdout.matchAll(/^\s*not ok \d+ - (.+?)\r?$/gm)]
    .map(match => match[1].trim()).filter(name => !/ #(?:TODO|SKIP)\b/i.test(name)).slice(0, 10);
  return { command: 'node ' + args.join(' '), node: process.version, exitCode: run.status, counts, failures };
}

export function replayTests(repo, head, files) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-diary-'));
  try {
    const archive = path.join(dir, 'source.tar'), snapshot = path.join(dir, 'source');
    fs.mkdirSync(snapshot);
    git(repo, ['archive', '--format=tar', '--output=' + archive, head]);
    execFileSync('tar', ['-xf', archive, '-C', snapshot], { windowsHide: true, timeout: 30000 });
    return runNodeTests(snapshot, files);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

export function describeRun(label, sha, receipt) {
  const { tests, pass, fail, cancelled, skipped, todo } = receipt.counts;
  const lines = [label + ' · ' + sha + '\n' + receipt.command + ' (' + receipt.node + ')',
    '종료 코드 ' + receipt.exitCode + ' · 전체 ' + tests + ' · 통과 ' + pass + ' · 실패 ' + fail + ' · 중단 ' + cancelled + ' · 건너뜀 ' + skipped + ' · TODO ' + todo];
  if (receipt.failures.length) lines.push('실패한 검사(최대 10개) · ' + receipt.failures.join(' / '));
  return lines.join('\n');
}
