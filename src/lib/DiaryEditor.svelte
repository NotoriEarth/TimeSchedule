<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { api, isNative } from './api';
  import type { EventInput, NoteInput, TaskInput } from './api';
  import { diaryApi, kindNames } from './diary-api';
  import type { EntryInput, Project, ProjectInput } from './diary-api';
  import type { EditorRequest } from './editor-types';
  let { request, projects, onclose, onsaved }: { request: EditorRequest; projects: Project[]; onclose: () => void; onsaved: (date?: string) => void } = $props();
  let dialog: HTMLDialogElement;
  // A keyed editor instance owns its draft until save or an explicit discard.
  const initialRequest = untrack(() => JSON.parse(JSON.stringify(request)) as EditorRequest);
  let event = $state<EventInput>(initialRequest.type === 'event' ? structuredClone(initialRequest.value) : {title:'', date:initialRequest.date, allDay:true,startTime:null,endTime:null,notes:'',color:'sage',completed:false});
  let entry = $state<EntryInput>(initialRequest.type === 'entry' ? structuredClone(initialRequest.value) : {title:'',date:initialRequest.date,body:'',kind:'note',projectId:null,sourceUrl:''});
  let project = $state<ProjectInput>(initialRequest.type === 'project' ? structuredClone(initialRequest.value) : {title:'',description:'',repoPath:''});
  let note = $state<NoteInput>(initialRequest.type === 'note' ? structuredClone(initialRequest.value) : {title:'',body:''});
  let task = $state<TaskInput>(initialRequest.type === 'task' ? structuredClone(initialRequest.value) : {title:'',date:initialRequest.date,completed:false});
  const initial = JSON.stringify({event,entry,project,note,task});
  let busy = $state(false);
  let error = $state('');
  let confirm = $state<'discard' | 'delete' | null>(null);
  const dirty = $derived(JSON.stringify({event,entry,project,note,task}) !== initial);
  const id = $derived(request.type === 'event' ? event.id : request.type === 'entry' ? entry.id : request.type === 'project' ? project.id : request.type === 'note' ? note.id : task.id);
  const name = $derived(request.type === 'event' ? '일정' : request.type === 'entry' ? '다이어리 기록' : request.type === 'project' ? '프로젝트' : request.type === 'note' ? '메모' : '할 일');
  function cancel() { if (busy) return; if (dirty) confirm='discard'; else onclose(); }
  async function save(e: SubmitEvent) {
    e.preventDefault(); if (busy) return; busy=true; error='';
    try {
      if (request.type==='event') await api.saveEvent(event);
      else if (request.type==='entry') await diaryApi.saveEntry(entry);
      else if (request.type==='project') await diaryApi.saveProject(project);
      else if (request.type==='note') await api.saveNote(note);
      else await api.saveTask(task);
      onsaved(request.type==='event' ? event.date : request.type==='entry' ? entry.date : undefined); onclose();
    } catch(cause) { error=String(cause); } finally { busy=false; }
  }
  async function remove() {
    if (!id || busy) return; if (confirm!=='delete') {confirm='delete'; return;} busy=true; error='';
    try {
      if(request.type==='event') await api.deleteEvent(id);
      else if(request.type==='entry') await diaryApi.deleteEntry(id);
      else if(request.type==='project') await diaryApi.deleteProject(id);
      else if(request.type==='note') await api.deleteNote(id);
      else await api.deleteTask(id);
      onsaved(); onclose();
    } catch(cause) {error=String(cause);} finally {busy=false;}
  }
  onMount(() => {
    dialog.showModal(); dialog.querySelector<HTMLInputElement>('input')?.focus();
    const unload=(e:BeforeUnloadEvent)=>{if(dirty || busy){e.preventDefault();e.returnValue='';}};
    window.addEventListener('beforeunload',unload);
    let disposed=false; let unlisten: (()=>void)|undefined;
    if (isNative) {
      void invoke('set_editor_open',{open:true}).catch(cause=>error=String(cause));
      void listen('editor-close-requested',cancel).then(fn=>{if(disposed)fn();else unlisten=fn;});
    }
    return ()=>{disposed=true;unlisten?.();window.removeEventListener('beforeunload',unload);if(isNative) void invoke('set_editor_open',{open:false});};
  });
</script>

<dialog bind:this={dialog} class="diary-editor" aria-labelledby="diary-editor-title" oncancel={(e)=>{e.preventDefault();cancel();}}>
  <form onsubmit={save}>
    <header><div><small>내 기록을 한 장씩</small><h2 id="diary-editor-title">{name} {id ? '수정' : '추가'}</h2></div><button type="button" aria-label="편집 닫기" onclick={cancel} disabled={busy}>✕</button></header>
    <fieldset disabled={busy}>
      {#if request.type==='project'}
        <label>프로젝트 이름<input required maxlength="200" bind:value={project.title} /></label>
        <label>설명<textarea rows="7" maxlength="20000" bind:value={project.description} placeholder="목표, 역할, 기술, 배운 점을 자유롭게 적어 보세요."></textarea></label>
        <label>로컬 Git 저장소 경로<input maxlength="2000" bind:value={project.repoPath} placeholder="C:\Projects\my-project" /></label>
        <p class="hint">경로를 저장한 뒤 프로젝트 화면에서 Git 수집을 실행합니다. 저장소 파일은 변경하지 않습니다.</p>
      {:else if request.type==='entry'}
        <label>제목<input required maxlength="200" bind:value={entry.title} /></label>
        <div class="editor-columns"><label>날짜<input required type="date" min="1900-01-01" max="9999-12-31" bind:value={entry.date} /></label><label>기록 종류<select bind:value={entry.kind}>{#each Object.entries(kindNames) as [value,label]}<option {value}>{label}</option>{/each}</select></label></div>
        <label>프로젝트<select bind:value={entry.projectId}><option value={null}>프로젝트 없음</option>{#each projects as p}<option value={p.id}>{p.title}</option>{/each}</select></label>
        <label>본문<textarea rows="13" maxlength="100000" bind:value={entry.body} placeholder={entry.kind==='troubleshooting' ? '문제와 증상\n시도한 방법\n원인과 해결\n검증 결과와 남은 일' : '오늘의 기록을 자유롭게 적어 보세요.'}></textarea></label>
        <label>출처 링크 (선택)<input type="url" maxlength="2000" bind:value={entry.sourceUrl} placeholder="https://…" /></label>
        {#if entry.kind==='summary' || entry.kind==='portfolio'}<p class="hint">근거 정리 초안입니다. 외부 AI로 생성한 결과가 아니며 직접 수정해 저장할 수 있습니다.</p>{/if}
      {:else if request.type==='event'}
        <label>제목<input required maxlength="200" bind:value={event.title} /></label>
        <label>날짜<input type="date" required min="1900-01-01" max="9999-12-31" bind:value={event.date} /></label>
        <label class="check"><input type="checkbox" bind:checked={event.allDay} onchange={()=>{if(!event.allDay){event.startTime||='09:00';event.endTime||='10:00';}}} />하루 종일</label>
        {#if !event.allDay}<div class="editor-columns"><label>시작<input type="time" required bind:value={event.startTime} /></label><label>종료<input type="time" required bind:value={event.endTime} /></label></div>{/if}
        <label>색상<select bind:value={event.color}><option value="sage">세이지</option><option value="blue">블루</option><option value="amber">앰버</option><option value="rose">로즈</option></select></label>
        <label>메모<textarea rows="7" maxlength="20000" bind:value={event.notes}></textarea></label>
        <label class="check"><input type="checkbox" bind:checked={event.completed} />완료한 일정</label>
      {:else if request.type==='note'}
        <label>제목<input required maxlength="200" bind:value={note.title} /></label><label>본문<textarea rows="13" maxlength="20000" bind:value={note.body}></textarea></label>
      {:else}
        <label>할 일<input required maxlength="200" bind:value={task.title} /></label><label>날짜 (선택)<input type="date" min="1900-01-01" max="9999-12-31" value={task.date??''} oninput={(e)=>task.date=e.currentTarget.value||null} /></label><label class="check"><input type="checkbox" bind:checked={task.completed} />완료</label>
      {/if}
      {#if error}<p class="diary-error" role="alert">{error}</p>{/if}
      {#if confirm}<div class="discard-box" role="alert"><p>{confirm==='discard' ? '저장하지 않은 내용을 버리고 닫을까요?' : request.type==='project' ? '프로젝트를 삭제할까요? 연결된 기록은 프로젝트 없이 보존됩니다.' : '삭제한 항목은 되돌릴 수 없습니다. 삭제할까요?'}</p><button class="paper-button" type="button" onclick={()=>confirm=null}>계속 편집</button><button class="paper-button danger" type="button" onclick={()=>confirm==='discard'?onclose():remove()}>{confirm==='discard'?'내용 버리고 닫기':'삭제 확인'}</button></div>{/if}
    </fieldset>
    <footer>{#if id}<button type="button" class="danger" onclick={remove} disabled={busy}>삭제</button>{/if}<span>{dirty?'저장되지 않은 변경':''}</span><button class="paper-button" type="button" onclick={cancel} disabled={busy}>취소</button><button class="paper-button primary" type="submit" disabled={busy}>{busy?'저장 중…':'저장하기'}</button></footer>
  </form>
</dialog>
