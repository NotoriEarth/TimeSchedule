import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

// Windows can name one folder by its long path or an 8.3 short one
// (C:\Users\RUNNER~1\...). The native resolver returns the long form Git also
// reports; the JS resolver keeps short names. Elsewhere the native resolver can
// fail without /proc and short names do not exist, so the JS one is used.
export const canonicalPath = value => process.platform === 'win32' ? fs.realpathSync.native(value) : fs.realpathSync(value);

const COMMIT = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;
const DOCUMENT = /(?:^|\/)(?:docs?|notes?|research)(?:\/|$)|\.(?:md|mdx|txt|rst|adoc)$/i;
const DEVELOPMENT = /^(?:apps\/|crates\/|src\/|src-tauri\/|scripts\/|tests\/|vendor\/|\.github\/workflows\/|Cargo\.(?:toml|lock)$|(?:package(?:-lock)?|tsconfig(?:\.[^/]+)?)\.json$|(?:svelte|vite|vitest|tailwind|postcss|eslint)\.config\.[^/]+$|(?:pnpm-lock\.yaml|yarn\.lock|rust-toolchain(?:\.toml)?|\.gitignore|\.gitattributes)$)/;

export const isDevelopmentPath = file => DEVELOPMENT.test(file) && !DOCUMENT.test(file);

// Git answers a definite "no" for `cat-file -e` (no such object) and
// `merge-base --is-ancestor` (not an ancestor) by exiting 1 without printing
// anything. Every other failure (Git not found, a timeout, exit 128 for an
// unreadable or locked repository, a warning on stderr) says nothing about the
// selected work, so it is thrown for the caller to retry. Exit codes are used
// instead of messages because Git translates its messages.
function git(repo, args, { answersNo = false } = {}) {
  try {
    return execFileSync('git', ['-C', repo, ...args], {
      encoding: 'utf8', windowsHide: true, timeout: 10000,
      maxBuffer: 2 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'],
      // While this repository's .git is briefly unreadable, Git must not find
      // an enclosing repository instead and report it as the top level.
      env: { ...process.env, GIT_CEILING_DIRECTORIES: path.dirname(repo) },
    });
  } catch (error) {
    if (answersNo && error.status === 1 && !error.signal && !String(error.stderr ?? '').trim()) return null;
    throw error;
  }
}
const describe = error => {
  const what = error.code ?? (error.signal ? `signal ${error.signal}` : error.status != null ? `exit ${error.status}` : error.name);
  const line = String(error.stderr ?? '').split(/\r?\n/).find(l => l.trim()) ?? '';
  return [what, line.trim()].filter(Boolean).join(': ').slice(0, 300);
};

// This checks the explicitly selected work, never unrelated dirty files or
// whichever commits happen to be at HEAD when an automatic retry runs.
export function developmentEvidence(repoPath, evidence, projectRoot) {
  const skip = reason => ({ eligible: false, reason });
  if (!evidence) return skip('missing_git_changes');
  const { base, head, paths } = evidence;
  if (!COMMIT.test(base ?? '') || !COMMIT.test(head ?? '') || !Array.isArray(paths) || !paths.length || paths.length > 1000
      || paths.some(p => typeof p !== 'string' || !p || p.length > 4096 || /[\\\0\r\n]/.test(p)
        || path.posix.isAbsolute(p) || p.includes(':') || p.split('/').some(part => !part || part === '.' || part === '..'))) {
    return skip('invalid_git_changes');
  }
  if (base === head) return skip('no_development_changes');
  const selected = [...new Set(paths)].sort();
  if (!selected.some(isDevelopmentPath)) return skip('no_development_changes');
  try {
    const canonical = canonicalPath;
    const repo = canonical(repoPath);
    const samePath = (a, b) => process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
    if (!samePath(repo, canonical(projectRoot))
        || !samePath(repo, canonical(git(repo, ['rev-parse', '--show-toplevel']).trim()))) return skip('wrong_repository');
    for (const sha of [base, head]) {
      if (git(repo, ['cat-file', '-e', sha], { answersNo: true }) === null) return skip('commit_not_found');
      if (git(repo, ['cat-file', '-t', sha]).trim() !== 'commit') return skip('invalid_git_changes');
    }
    if (git(repo, ['merge-base', '--is-ancestor', base, head], { answersNo: true }) === null) return skip('not_ancestor');
    const changed = new Set(git(repo, ['diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', base, head, '--']).split('\0').filter(Boolean));
    if (!selected.every(p => changed.has(p))) return skip('unchanged_selected_paths');
    return { eligible: true, gitChanges: { base, head, paths: selected } };
  } catch (error) {
    // Git or the folder could not answer. Nothing is known about the selected
    // work yet: no native or model side effects now, and the caller retries.
    return { eligible: false, reason: 'git_unavailable', transient: true, detail: describe(error) };
  }
}
