import { invoke } from '@tauri-apps/api/core';
import { isNative, readPreview, writePreview, saveRecord, deleteRecord, validateDate } from './api';

export type Project = { id: string; title: string; description: string; repoPath: string; updatedAt: string };
export type ProjectInput = Omit<Project, 'id' | 'updatedAt'> & { id?: string };
export type EntryKind = 'note' | 'development' | 'troubleshooting' | 'commit' | 'summary' | 'portfolio';
export type DiaryEntry = { id: string; projectId: string | null; date: string; kind: EntryKind; title: string; body: string; sourceUrl: string; sourceRef: string | null; sourceBody: string; updatedAt: string };
export type EntryInput = Omit<DiaryEntry, 'id' | 'updatedAt' | 'sourceRef' | 'sourceBody'> & { id?: string };
export type EntrySummary = Pick<DiaryEntry, 'id' | 'projectId' | 'date' | 'kind' | 'title' | 'updatedAt'>;
export type SearchHit = { id: string; kind: 'event' | 'task' | 'note' | 'project' | 'entry'; title: string; excerpt: string; date: string | null; projectId: string | null };
export const kindNames: Record<EntryKind, string> = { note: '자유 노트', development: '개발 기록', troubleshooting: '트러블슈팅', commit: 'Git 커밋', summary: '근거 요약', portfolio: '포트폴리오 초안' };
export function safeLink(value: string): string | null {
  try { const url = new URL(value); return ['http:', 'https:'].includes(url.protocol) ? url.href : null; } catch { return null; }
}
function text(value: string, max: number, label: string): void {
  if ([...value].length > max || value.includes('\0')) throw new Error(`${label}: ${max.toLocaleString()}자 이내로 입력해 주세요.`);
}
function title(value: string): void { if (!value.trim()) throw new Error('제목을 입력해 주세요.'); text(value.trim(), 200, '제목'); }
function summary(e: DiaryEntry): EntrySummary { return { id:e.id, projectId:e.projectId,date:e.date,kind:e.kind,title:e.title,updatedAt:e.updatedAt }; }

export const diaryApi = {
  async openSource(url: string): Promise<void> {
    const safe=safeLink(url); if (!safe) throw new Error('올바른 http 또는 https 출처 주소가 아닙니다.');
    if (isNative) return invoke('open_source_url',{url:safe});
    window.open(safe,'_blank','noopener,noreferrer');
  },
  async projects(): Promise<Project[]> { return isNative ? invoke('list_projects') : readPreview().projects.sort((a,b) => b.updatedAt.localeCompare(a.updatedAt)); },
  async saveProject(input: ProjectInput): Promise<Project> {
    title(input.title); text(input.description,20000,'설명'); text(input.repoPath,2000,'경로');
    const clean = {...input,title:input.title.trim(),repoPath:input.repoPath.trim()};
    return isNative ? invoke('save_project',{input:clean}) : saveRecord<Project>('projects',clean);
  },
  async deleteProject(id: string): Promise<void> {
    if (isNative) return invoke('delete_project',{id});
    const db=readPreview(); if (!db.projects.some(p=>p.id===id)) throw new Error('프로젝트를 찾을 수 없습니다.');
    db.projects=db.projects.filter(p=>p.id!==id); db.entries=db.entries.map(e=>e.projectId===id?{...e,projectId:null}:e); writePreview(db);
  },
  async entries(from: string, to: string, projectId: string | null = null, offset=0): Promise<EntrySummary[]> {
    if (isNative) return invoke('list_entries',{from,to,projectId,offset});
    return readPreview().entries.filter(e=>e.date>=from && e.date<=to && (!projectId || e.projectId===projectId)).sort((a,b)=>b.date.localeCompare(a.date)||b.updatedAt.localeCompare(a.updatedAt)||a.id.localeCompare(b.id)).slice(offset,offset+200).map(summary);
  },
  async entry(id: string): Promise<DiaryEntry> {
    if (isNative) return invoke('get_entry',{id});
    const e=readPreview().entries.find(e=>e.id===id); if (!e) throw new Error('기록을 찾을 수 없습니다.'); return e;
  },
  async saveEntry(input: EntryInput): Promise<DiaryEntry> {
    title(input.title); text(input.body,100000,'기록'); text(input.sourceUrl,2000,'출처');
    if (!validateDate(input.date)) throw new Error('올바른 날짜를 선택해 주세요.');
    if (!(input.kind in kindNames)) throw new Error('기록 종류를 확인해 주세요.');
    if (input.sourceUrl && !safeLink(input.sourceUrl)) throw new Error('출처는 http 또는 https 주소로 입력해 주세요.');
    const clean={...input,title:input.title.trim()};
    if (isNative) return invoke('save_entry',{input:clean});
    const db=readPreview();
    if (input.projectId && !db.projects.some(p=>p.id===input.projectId)) throw new Error('프로젝트를 찾을 수 없습니다.');
    const old=db.entries.find(e=>e.id===input.id);
    return saveRecord<DiaryEntry>('entries',{...clean,sourceRef:old?.sourceRef??null,sourceBody:old?.sourceBody??''});
  },
  async deleteEntry(id: string): Promise<void> { return isNative ? invoke('delete_entry',{id}) : deleteRecord('entries',id); },
  async search(query: string): Promise<SearchHit[]> {
    if (isNative) return invoke('search_diary',{query,limit:50});
    const needle=query.trim().toLowerCase(); if (!needle) return []; text(needle,200,'검색어');
    const db=readPreview();
    const rows: (SearchHit & {updatedAt:string; body:string})[] = [
      ...db.events.map(e=>({id:e.id,kind:'event' as const,title:e.title,body:e.notes,date:e.date,projectId:null,updatedAt:e.updatedAt,excerpt:''})),
      ...db.tasks.map(t=>({id:t.id,kind:'task' as const,title:t.title,body:'',date:t.date,projectId:null,updatedAt:t.updatedAt,excerpt:''})),
      ...db.notes.map(n=>({...n,kind:'note' as const,date:null,projectId:null,excerpt:''})),
      ...db.projects.map(p=>({...p,kind:'project' as const,body:p.description,date:null,projectId:p.id,excerpt:''})),
      ...db.entries.map(e=>({...e,kind:'entry' as const,excerpt:''})),
    ];
    return rows.filter(r=>r.title.toLowerCase().includes(needle)||r.body.toLowerCase().includes(needle)).sort((a,b)=>b.updatedAt.localeCompare(a.updatedAt)).slice(0,50).map(({body,updatedAt,...r})=>({...r,excerpt:body.slice(0,160)}));
  },
  async importGit(projectId: string): Promise<{ imported: number; scanned: number; limited: boolean }> {
    if (!isNative) throw new Error('Git 수집은 데스크톱 앱에서 사용할 수 있습니다.');
    return invoke('import_git',{projectId});
  },
};
