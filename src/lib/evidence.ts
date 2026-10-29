export type Evidence = { id: string; title: string; date: string; body: string; sourceUrl?: string; sourceRef?: string | null };

/** Deterministic source outline. Never invents outcomes and never writes an existing note. */
export function evidenceDraft(scope: string, records: Evidence[], portfolio = false, truncated = false): string {
  const lines = [portfolio ? `${scope} · 포트폴리오 근거 초안` : `${scope} · 근거 요약 초안`,
    '저장된 자료를 날짜순으로 정리한 초안입니다. AI 분석 결과가 아니며, 성과와 배운 점은 직접 확인해 작성해 주세요.', ''];
  if (!records.length) lines.push('이 범위에 저장된 근거가 없습니다.');
  for (const record of [...records].sort((a,b)=>a.date.localeCompare(b.date))) {
    lines.push(`${record.date} · ${record.title}`, `기록 ID: ${record.id}`);
    if (record.body) lines.push(record.body.slice(0, 600) + (record.body.length > 600 ? '\n[본문 발췌 · 원문에서 계속]' : ''));
    if (record.sourceRef) lines.push(`출처: ${record.sourceRef}`);
    if (record.sourceUrl) lines.push(record.sourceUrl);
    lines.push('');
  }
  if (truncated) lines.push('[최대 100개 근거의 초안입니다. 나머지 기록은 원문 목록에서 확인하세요.]', '');
  lines.push('직접 보완할 내용', portfolio ? '• 프로젝트 목적과 내가 맡은 역할\n• 문제, 시도, 해결 근거\n• 검증된 결과와 배운 점' : '• 이번 기간의 배운 점\n• 해결하지 못한 문제\n• 다음에 할 일');
  return lines.join('\n');
}
