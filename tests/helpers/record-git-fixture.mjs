import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

export function git(repo, ...args) {
  return execFileSync('git', ['-c', `core.excludesFile=${process.platform === 'win32' ? 'NUL' : '/dev/null'}`, '-C', repo, ...args], {
    encoding: 'utf8', windowsHide: true,
    env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null' },
  }).trim();
}

export function gitFixture(directory) {
  const repo = path.join(directory, 'repo');
  fs.mkdirSync(repo, { recursive: true });
  git(repo, 'init', '-q');
  git(repo, 'config', 'user.name', 'Record Test');
  git(repo, 'config', 'user.email', 'record-test@example.invalid');
  git(repo, 'config', 'commit.gpgsign', 'false');
  function commit(file, text) {
    fs.mkdirSync(path.dirname(path.join(repo, file)), { recursive: true });
    fs.writeFileSync(path.join(repo, file), text);
    git(repo, 'add', '--', file);
    git(repo, 'commit', '-qm', 'fixture');
    return git(repo, 'rev-parse', 'HEAD');
  }
  const base = commit('README.md', 'Initial fixture\n');
  const docs = commit('docs/research.md', 'Search and price comparison only\n');
  const head = commit('src/example.js', 'export const answer = 42;\n');
  return { repo, base, docs, head, commit, gitChanges: { base: docs, head, paths: ['src/example.js'] } };
}
