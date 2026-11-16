import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const COMMIT = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;
const DOCUMENT = /(?:^|\/)(?:docs?|notes?|research)(?:\/|$)|\.(?:md|mdx|txt|rst|adoc)$/i;
const DEVELOPMENT = /^(?:apps\/|crates\/|src\/|src-tauri\/|scripts\/|tests\/|vendor\/|\.github\/workflows\/|Cargo\.(?:toml|lock)$|(?:package(?:-lock)?|tsconfig(?:\.[^/]+)?)\.json$|(?:svelte|vite|vitest|tailwind|postcss|eslint)\.config\.[^/]+$|(?:pnpm-lock\.yaml|yarn\.lock|rust-toolchain(?:\.toml)?|\.gitignore|\.gitattributes)$)/;

function git(repo, ...args) {
  return execFileSync('git', ['-C', repo, ...args], {
    encoding: 'utf8', windowsHide: true, timeout: 10000,
    maxBuffer: 2 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'],
  });
}

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
  if (!selected.some(p => DEVELOPMENT.test(p) && !DOCUMENT.test(p))) return skip('no_development_changes');
  try {
    // The native resolver expands Windows 8.3 short names (C:\Users\RUNNER~1\...)
    // to the long form Git reports; the JS one keeps them, so the same repository
    // would compare unequal and every record would be skipped.
    const canonical = value => fs.realpathSync.native(value);
    const repo = canonical(repoPath);
    const samePath = (a, b) => process.platform === 'win32' ? a.toLowerCase() === b.toLowerCase() : a === b;
    if (!samePath(repo, canonical(projectRoot))
        || !samePath(repo, canonical(git(repo, 'rev-parse', '--show-toplevel').trim()))) return skip('wrong_repository');
    for (const sha of [base, head]) if (git(repo, 'cat-file', '-t', sha).trim() !== 'commit') return skip('invalid_git_changes');
    git(repo, 'merge-base', '--is-ancestor', base, head);
    const changed = new Set(git(repo, 'diff', '--name-only', '-z', '--no-renames', '--no-ext-diff', base, head, '--').split('\0').filter(Boolean));
    if (!selected.every(p => changed.has(p))) return skip('unchanged_selected_paths');
    return { eligible: true, gitChanges: { base, head, paths: selected } };
  } catch {
    // No native or model side effects when Git cannot establish the evidence.
    return skip('git_verification_failed');
  }
}
