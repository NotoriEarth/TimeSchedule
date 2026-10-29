import test from 'node:test';
import assert from 'node:assert/strict';
import { weekAfterSunday, weekContaining, addDays, calendarDays, monthStart } from '../src/lib/dates.ts';
import { evidenceDraft } from '../src/lib/evidence.ts';

test('week selector spans the following Monday through Sunday across year boundary', () => {
  assert.deepEqual(weekAfterSunday('2026-12-27'), { from: '2026-12-28', to: '2027-01-03' });
  assert.deepEqual(weekAfterSunday('2026-10-25'), { from: '2026-10-26', to: '2026-11-01' });
  assert.throws(() => weekAfterSunday('2026-10-26'));
});

test('evidence draft names sources without inventing outcomes or mutating records', () => {
  const records=[{id:'source-1',title:'한글 원문',date:'2026-10-29',body:'직접 작성한 해결 근거',sourceRef:'repo#abc'}];
  const original=JSON.stringify(records);
  const draft=evidenceDraft('프로젝트',records,true,true);
  assert.match(draft,/AI 분석 결과가 아니며/);
  assert.match(draft,/source-1/); assert.match(draft,/repo#abc/); assert.match(draft,/직접 작성한 해결 근거/);
  assert.match(draft,/최대 100개/); assert.equal(JSON.stringify(records),original);
  assert.match(evidenceDraft('빈 주간',[]),/근거가 없습니다/);
});
test('Sunday belongs to the preceding Monday and leap dates remain local', () => {
  assert.deepEqual(weekContaining('2027-01-03'), { from: '2026-12-28', to: '2027-01-03' });
  assert.equal(addDays('2028-02-28', 1), '2028-02-29');
  const days = calendarDays(monthStart('2026-10-15'));
  const week = weekAfterSunday('2026-10-25');
  assert.equal(days.filter(d => d.key >= week.from && d.key <= week.to).length, 7);
});
