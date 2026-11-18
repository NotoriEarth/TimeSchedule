import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { isDevelopmentPath } from './project-records-git.mjs';

export function git(repo, args, options = {}) {
  return execFileSync('git', ['-C', repo, ...args], {
    encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true,
    timeout: 30000, maxBuffer: 32 * 1024 * 1024, ...options,
  });
}

const filesChanged = (repo, base, head) => git(repo, ['diff', '--no-renames', '--name-only', '-z', base, head, '--']).split('\0').filter(Boolean);

export function inspectBranch(repo, start, id, head) {
  if (git(repo, ['merge-base', start.base, head]).trim() !== start.base) {
    throw new Error('작업 시작 커밋이 현재 이력에 없습니다. 브랜치의 귀속을 확인해 주세요.');
  }
  const commits = git(repo, ['rev-list', '--first-parent', '--reverse', start.base + '..' + head]).trim().split(/\s+/).filter(Boolean);
  const owned = [], unassigned = [], ambiguous = [];
  let mixed = false, needsProjection = false, lastDevelopment = 0;
  for (const sha of commits) {
    const format = '%P%x00%ae%x00%(trailers:key=Diary-Work-Id,valueonly)%x00%(trailers:key=Co-authored-by,valueonly)';
    const [parents, author, tags, coauthors] = git(repo, ['show', '-s', '--format=' + format, sha]).trimEnd().split('\0');
    const parent = parents.split(' ')[0];
    if (sha === commits[0] && parent !== start.base) throw new Error('작업 시작 커밋이 브랜치의 첫 부모 이력에서 바뀌었습니다. 귀속을 확인해 주세요.');
    const mine = author.trim().toLowerCase() === start.owner;
    const ids = tags.trim().split(/\r?\n/).filter(Boolean);
    if (mine && id && ids.length === 1 && ids[0] === id && !parents.includes(' ') && !coauthors?.trim()) {
      owned.push({ sha, parent });
      if (filesChanged(repo, parent, sha).some(isDevelopmentPath)) {
        lastDevelopment = owned.length;
        needsProjection = mixed;
      }
    } else {
      mixed = true;
      if (filesChanged(repo, parent, sha).some(isDevelopmentPath)) {
        if (mine && !ids.length) unassigned.push(sha);
        else if (id && ids.includes(id)) ambiguous.push(sha);
      }
    }
  }
  return { owned: owned.slice(0, lastDevelopment), assigned: owned.length, unassigned, ambiguous, needsProjection };
}

// Rebuild just the assigned patches on their original base. Neither the current
// index nor the working tree is touched. The private ref keeps retries verifiable.
export function gitEvidence(repo, directory, start, workId, scan) {
  if (!scan.owned.length) throw new Error('작성자와 작업 ID가 일치하는 커밋이 없습니다.');
  const last = scan.owned.at(-1).sha;
  let head = last;
  if (scan.needsProjection) {
    const temp = fs.mkdtempSync(path.join(directory, 'index-'));
    const env = { ...process.env, GIT_INDEX_FILE: path.join(temp, 'index') };
    try {
      git(repo, ['read-tree', start.base], { env });
      for (const { parent, sha } of scan.owned) {
        const patch = git(repo, ['diff', '--binary', '--full-index', '--no-ext-diff', '--no-renames', parent, sha, '--'], { encoding: 'buffer' });
        if (patch.length) {
          try { git(repo, ['apply', '--cached', '--whitespace=nowarn', '-'], { env, input: patch }); }
          catch (error) { throw new Error('작업 패치를 독립적으로 적용하지 못했습니다. 커밋 귀속과 Git 오류를 확인하세요. ' + sha, { cause: error }); }
        }
      }
      const tree = git(repo, ['write-tree'], { env }).trim();
      const date = git(repo, ['show', '-s', '--format=%cI', last]).trim();
      const identity = { ...env, GIT_AUTHOR_NAME: 'TimeSchedule diary', GIT_AUTHOR_EMAIL: 'diary@localhost',
        GIT_COMMITTER_NAME: 'TimeSchedule diary', GIT_COMMITTER_EMAIL: 'diary@localhost', GIT_AUTHOR_DATE: date, GIT_COMMITTER_DATE: date };
      head = git(repo, ['commit-tree', tree, '-p', start.base], { env: identity, input: 'Diary evidence ' + workId + '\n' + last + '\n' }).trim();
      git(repo, ['update-ref', 'refs/diary-evidence/' + workId + '/' + last, head]);
    } finally {
      fs.rmSync(temp, { recursive: true, force: true });
    }
  }
  return { base: start.base, head, paths: filesChanged(repo, start.base, head).filter(isDevelopmentPath).sort() };
}
