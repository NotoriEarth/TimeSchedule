<script lang="ts">
  import { onMount } from 'svelte';
  import { api, isNative, onAppError, onDataChange, onDesktopChange, onSelectDate } from './api';
  import type { CalendarEvent, Task, Note, AppInfo } from './api';
  import { diaryApi, kindNames } from './diary-api';
  import type { DiaryEntry, EntryKind, EntrySummary, Project, SearchHit } from './diary-api';
  import { addDays, calendarDays, dateKey, monthStart, parseDate, readableDate, shiftMonth, weekdayNames, weekAfterSunday, weekContaining } from './dates';
  import { evidenceDraft } from './evidence';
  import type { Evidence } from './evidence';
  import type { EditorRequest } from './editor-types';
  import EntryCard from './EntryCard.svelte';
  import DiaryEditor from './DiaryEditor.svelte';
  import './diary.css';

  type View = 'month' | 'week' | 'projects' | 'records' | 'tasks' | 'notes' | 'settings';
  const queryDate = new URLSearchParams(window.location.search).get('date');
  const initialDate = queryDate && /^\d{4}-\d{2}-\d{2}$/.test(queryDate) && dateKey(parseDate(queryDate))===queryDate ? queryDate : dateKey();
  let selected = $state(initialDate);
  let month = $state(monthStart(initialDate));
  let week = $state(weekContaining(initialDate));
  let view = $state<View>('month');
  let hoverWeek = $state<{from:string;to:string}|null>(null);
  let projects = $state<Project[]>([]);
  let projectId = $state<string|null>(null);
  let events = $state<CalendarEvent[]>([]);
  let tasks = $state<Task[]>([]);
  let notes = $state<Note[]>([]);
  let entries = $state<EntrySummary[]>([]);
  let more = $state(false);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');
  let appInfo = $state<AppInfo|null>(null);
  let editor = $state<EditorRequest|null>(null);
  let editorKey = $state(0);
  let search = $state('');
  let results = $state<SearchHit[]>([]);
  let searching = $state(false);
  let searchError = $state('');
  let searchVersion = 0;
  let loadVersion = 0;
  let disposed = false;
  const days = $derived(calendarDays(month));
  const monthLabel = $derived(`${month.getFullYear()}년 ${month.getMonth()+1}월`);
  const weekDays = $derived(Array.from({length:7},(_,i)=>addDays(week.from,i)));
  const activeProject = $derived(projects.find(p=>p.id===projectId)??null);
  const title = $derived(view==='month' ? monthLabel : view==='week' ? `${week.from} — ${week.to}` : view==='projects' ? activeProject?.title??'나의 프로젝트' : view==='records' ? '기록 보관함' : view==='tasks' ? '나의 할 일' : view==='notes' ? '나의 메모' : '다이어리 설정');

  function entryRange() { return view==='projects'||view==='records' ? {from:'1900-01-01',to:'9999-12-31'} : view==='week' ? week : {from:days[0].key,to:days[41].key}; }
  async function refresh() {
    const version=++loadVersion; loading=true;
    const range=entryRange(); const project=view==='projects'?projectId:null;
    const eventRange=view==='week'?week:{from:days[0].key,to:days[41].key};
    try {
      const [p,e,t,n,r]=await Promise.all([diaryApi.projects(),api.listEvents(eventRange.from,eventRange.to),api.listTasks(),api.listNoteHeaders(),diaryApi.entries(range.from,range.to,project)]);
      if(disposed||version!==loadVersion)return;
      projects=p;events=e;tasks=t;notes=n;entries=r;more=r.length===200;
      if(projectId && !p.some(p=>p.id===projectId)) projectId=null;
    }catch(cause){if(version===loadVersion)error=String(cause);}finally{if(version===loadVersion)loading=false;}
  }
  async function loadMore() {
    const version=loadVersion; const range=entryRange();
    busy=true;
    try { const next=await diaryApi.entries(range.from,range.to,view==='projects'?projectId:null,entries.length);if(version===loadVersion){entries=[...entries,...next];more=next.length===200;} }catch(cause){error=String(cause);}finally{busy=false;}
  }
  function navigate(next:View){view=next;search='';notice='';void refresh();}
  function changeMonth(delta:number){month=shiftMonth(month,delta);hoverWeek=null;void refresh();}
  function openWeek(sunday:string){week=weekAfterSunday(sunday);selected=week.from;view='week';hoverWeek=null;void refresh();}
  function moveWeek(delta:number){week={from:addDays(week.from,delta*7),to:addDays(week.to,delta*7)};selected=week.from;void refresh();}
  function showToday(){selected=dateKey();month=monthStart(selected);week=weekContaining(selected);void refresh();}
  function openEditor(request:EditorRequest){if(editor){notice='현재 편집을 먼저 마쳐 주세요.';return;}editorKey++;editor=request;}
  function newEvent(date=selected){selected=date;openEditor({type:'event',date,value:{title:'',date,allDay:true,startTime:null,endTime:null,notes:'',color:'sage',completed:false}});}
  function editEvent(e:CalendarEvent){openEditor({type:'event',date:e.date,value:{...e}});}
  function newEntry(date=selected,kind:EntryKind='development'){selected=date;openEditor({type:'entry',date,value:{title:'',date,kind,body:'',projectId:view==='projects'?projectId:null,sourceUrl:''}});}
  function editEntry(e:DiaryEntry){openEditor({type:'entry',date:e.date,value:{...e}});}
  function editProject(p?:Project){openEditor({type:'project',date:selected,value:p?{...p}:{title:'',description:'',repoPath:''}});}
  function newTask(){openEditor({type:'task',date:selected,value:{title:'',date:selected,completed:false}});}
  function saved(date?:string){if(date)selected=date;notice='내 컴퓨터에 저장했습니다.';void refresh();}
  async function attempt(action:()=>Promise<unknown>){error='';try{await action();}catch(cause){error=String(cause);}}
  async function editNote(id?:string){await attempt(async()=>openEditor({type:'note',date:selected,value:id?await api.getNote(id):{title:'',body:''}}));}
  async function toggleTask(task:Task){await attempt(async()=>{await api.saveTask({...task,completed:!task.completed});await refresh();});}
  function chooseProject(id:string|null){projectId=id;view='projects';search='';void refresh();}
  async function importGit(){
    if(!projectId||busy)return;busy=true;notice='';
    await attempt(async()=>{const result=await diaryApi.importGit(projectId!);notice=`커밋 ${result.scanned}개 확인 · ${result.imported}개 추가 · ${result.scanned-result.imported}개 중복 보존${result.limited?' (최근 500개 제한)':''}`;await refresh();});busy=false;
  }
  async function draft(portfolio=false){
    if(busy)return;busy=true;
    const scopeView=view, scopeProject=projectId, scopeSelected=selected, scopeEditor=editorKey, scopeVersion=loadVersion;
    const scopeWeek={...week}, projectTitle=activeProject?.title, projectDescription=activeProject?.description;
    await attempt(async()=>{
      const range=scopeView==='projects'?{from:'1900-01-01',to:'9999-12-31'}:scopeWeek;
      const summaries=await diaryApi.entries(range.from,range.to,scopeView==='projects'?scopeProject:null);
      const eligible=summaries.filter(e=>e.kind!=='summary'&&e.kind!=='portfolio');
      const full=await Promise.all(eligible.slice(0,100).map(e=>diaryApi.entry(e.id)));
      let records:Evidence[]=full.map(e=>({...e}));
      if(scopeView!=='projects'){
        const scheduled=await api.listEvents(range.from,range.to);
        records=[...records,...scheduled.map(e=>({id:e.id,title:e.title,date:e.date,body:e.notes})),...tasks.filter(t=>t.date&&t.date>=range.from&&t.date<=range.to).map(t=>({id:t.id,title:t.title,date:t.date!,body:t.completed?'완료 표시됨':'미완료'}))];
      }
      if(disposed || view!==scopeView || projectId!==scopeProject || editorKey!==scopeEditor || loadVersion!==scopeVersion)return;
      const scope=scopeView==='projects'?(projectTitle??'전체 프로젝트'):`${range.from} ~ ${range.to}`;
      let body=evidenceDraft(scope,records.slice(0,100),portfolio,summaries.length===200||eligible.length>100||records.length>100);
      if(scopeView==='projects' && projectDescription)body=`프로젝트 설명\n${projectDescription}\n\n${body}`;
      if(body.length>99000)body=body.slice(0,99000)+'\n[길이 제한으로 발췌되었습니다. 원문 기록을 확인하세요.]';
      openEditor({type:'entry',date:scopeSelected,value:{title:`${scope} · ${portfolio?'포트폴리오 초안':'근거 요약'}`.slice(0,200),date:scopeSelected,kind:portfolio?'portfolio':'summary',body,projectId:scopeView==='projects'?scopeProject:null,sourceUrl:''}});
    });busy=false;
  }
  async function openResult(hit:SearchHit){
    await attempt(async()=>{
      if(hit.kind==='project')chooseProject(hit.id);
      else if(hit.kind==='entry')editEntry(await diaryApi.entry(hit.id));
      else if(hit.kind==='note')await editNote(hit.id);
      else if(hit.kind==='task'){const t=(await api.listTasks()).find(t=>t.id===hit.id);if(!t)throw new Error('할 일을 찾을 수 없습니다.');openEditor({type:'task',date:t.date??selected,value:t});}
      else if(hit.date){const e=(await api.listEvents(hit.date,hit.date)).find(e=>e.id===hit.id);if(!e)throw new Error('일정을 찾을 수 없습니다.');selected=e.date;month=monthStart(e.date);view='month';void refresh();editEvent(e);}
      search='';
    });
  }
  $effect(()=>{
    const query=search;const version=++searchVersion;results=[];searchError='';searching=!!query.trim();
    if(!query.trim())return;
    const timer=setTimeout(()=>{void diaryApi.search(query).then(found=>{if(version===searchVersion){results=found;searching=false;}}).catch(cause=>{if(version===searchVersion){searchError=String(cause);searching=false;}});},220);
    return()=>{clearTimeout(timer);};
  });
  onMount(()=>{
    const disposers:(()=>void)[]=[];
    void(async()=>{
      try{
        for(const subscribe of [()=>onDataChange(()=>{void refresh();}),()=>onSelectDate(date=>{if(editor){notice='편집을 마친 뒤 위젯 날짜를 다시 선택해 주세요.';return;}selected=date;month=monthStart(date);week=weekContaining(date);view='month';void refresh();newEvent(date);}),()=>onDesktopChange(()=>{void attempt(async()=>{appInfo=await api.getAppInfo();});}),()=>onAppError(message=>error=message)]){
          const dispose=await subscribe();if(disposed){dispose();return;}disposers.push(dispose);
        }
        appInfo=await api.getAppInfo();await refresh();
        if(queryDate && !disposed)newEvent(initialDate);
      }catch(cause){if(!disposed){error=String(cause);loading=false;}}
    })();
    return()=>{disposed=true;loadVersion++;searchVersion++;disposers.forEach(fn=>fn());};
  });
</script>

<div class="diary-shell" class:week-view={view==='week'}>
  <aside class="diary-sidebar">
    <a class="diary-brand" href="./" onclick={(e)=>{e.preventDefault();navigate('month');}}><span>TS</span><div>TimeSchedule<small>나의 만년 다이어리</small></div></a>
    <p class="nav-caption">하루를 기록하는 공간</p>
    <nav aria-label="다이어리 메뉴">
      {#each [{key:'month',label:'월간 다이어리',mark:'▦'},{key:'week',label:'주간 다이어리',mark:'☷'},{key:'projects',label:'프로젝트',mark:'◇'},{key:'records',label:'기록 보관함',mark:'▤'},{key:'tasks',label:'할 일',mark:'✓'},{key:'notes',label:'메모',mark:'✎'}] as item}
        <button class:active={view===item.key} onclick={()=>navigate(item.key as View)}><span aria-hidden="true">{item.mark}</span>{item.label}</button>
      {/each}
    </nav>
    <div class="sidebar-projects"><div><small>프로젝트</small><button aria-label="프로젝트 추가" onclick={()=>editProject()}>+</button></div>{#each projects.slice(0,8) as p}<button class:chosen={view==='projects'&&projectId===p.id} onclick={()=>chooseProject(p.id)}>{p.title}</button>{/each}{#if projects.length>8}<button onclick={()=>chooseProject(null)}>전체 {projects.length}개 보기</button>{/if}</div>
    <div class="diary-sidebar-bottom"><p>기억하고 싶은 하루를<br/>내 속도로 채워 보세요.</p><button onclick={()=>navigate('settings')}>⚙ 다이어리 설정</button><small>{isNative?'내 컴퓨터 · SQLite':'브라우저 미리보기'}</small></div>
  </aside>
  <main class="diary-main">
    <header class="diary-topbar"><span>오늘, {readableDate(dateKey())}</span><div class="diary-search"><label><span aria-hidden="true">⌕</span><input aria-label="다이어리 전체 검색" maxlength="200" bind:value={search} placeholder="일정, 프로젝트, 기록 검색" /></label>{#if search}<button aria-label="검색 닫기" onclick={()=>search=''}>✕</button>{/if}
      {#if search.trim()}<section class="search-results" aria-label="검색 결과" aria-live="polite"><small>{searching?'검색 중…':`${results.length}개 결과 · 최대 50개`}</small>{#if searchError}<p role="alert">{searchError}</p>{/if}{#each results as result}<button onclick={()=>openResult(result)}><span>{result.kind==='entry'?'기록':result.kind==='project'?'프로젝트':result.kind==='event'?'일정':result.kind==='task'?'할 일':'메모'} {result.date??''}</span><strong>{result.title}</strong><p>{result.excerpt}</p></button>{/each}{#if !searching&&!searchError&&!results.length}<p>검색어가 포함된 기록이 없습니다.</p>{/if}{#if results.length===50}<p>결과가 많습니다. 검색어를 더 구체적으로 입력해 주세요.</p>{/if}</section>{/if}
    </div></header>
    {#if !isNative}<p class="diary-preview">브라우저 미리보기 · 이 브라우저에만 저장되며 데스크톱 데이터와 별개입니다.</p>{/if}
    {#if error}<div class="diary-error" role="alert">{error}<button onclick={()=>{error='';void refresh();}}>다시 불러오기</button></div>{/if}
    {#if notice}<div class="diary-notice" role="status">{notice}<button aria-label="알림 닫기" onclick={()=>notice=''}>✕</button></div>{/if}
    <div class="diary-heading"><div><small>{view==='month'?'MONTHLY JOURNAL':view==='week'?'WEEKLY JOURNAL':'MY PERSONAL ARCHIVE'}</small><h1>{title}</h1></div><div class="diary-actions">
      {#if view==='month'}<button class="paper-button" aria-label="이전 달" onclick={()=>changeMonth(-1)}>‹</button><button class="paper-button" onclick={showToday}>오늘</button><button class="paper-button" aria-label="다음 달" onclick={()=>changeMonth(1)}>›</button><button class="paper-button primary" onclick={()=>newEvent()}>+ 일정</button>
      {:else if view==='week'}<button class="paper-button" onclick={()=>navigate('month')}>월간으로</button><button class="paper-button" aria-label="이전 주" onclick={()=>moveWeek(-1)}>‹</button><button class="paper-button" aria-label="다음 주" onclick={()=>moveWeek(1)}>›</button><button class="paper-button" onclick={()=>draft()} disabled={busy}>주간 근거 요약</button>
      {:else if view==='projects'}<button class="paper-button" onclick={()=>editProject()}>+ 프로젝트</button>{#if activeProject}<button class="paper-button" onclick={()=>editProject(activeProject!)}>프로젝트 수정</button>{/if}
      {:else if view==='records'}<button class="paper-button primary" onclick={()=>newEntry()}>+ 기록</button>
      {:else if view==='tasks'}<button class="paper-button primary" onclick={newTask}>+ 할 일</button>
      {:else if view==='notes'}<button class="paper-button primary" onclick={()=>editNote()}>+ 메모</button>{/if}
    </div></div>

    {#if view==='month'}
      <div class="month-paper">
        <div class="month-weekdays"><span>주간</span>{#each weekdayNames as name,i}<span class:sunday={i===0} class:saturday={i===6}>{name}</span>{/each}</div>
        <div class="month-grid">
          {#each Array.from({length:6},(_,i)=>i) as row}
            {@const range=weekAfterSunday(days[row*7].key)}
            <button class="week-select" aria-label={`${range.from}부터 ${range.to} 주간 열기`} title={`${range.from} — ${range.to}`} onmouseenter={()=>hoverWeek=range} onmouseleave={()=>hoverWeek=null} onfocus={()=>hoverWeek=range} onblur={()=>hoverWeek=null} onclick={()=>openWeek(days[row*7].key)}><span>W{row+1}</span><span>→</span></button>
            {#each days.slice(row*7,row*7+7) as day}
              {@const cellEvents=events.filter(e=>e.date===day.key)}{@const cellEntries=entries.filter(e=>e.date===day.key)}
              <button class="paper-day" class:outside={!day.currentMonth} class:today={day.key===dateKey()} class:selected={day.key===selected} class:week-highlight={hoverWeek&&day.key>=hoverWeek.from&&day.key<=hoverWeek.to} aria-label={`${readableDate(day.key)}, 일정 ${cellEvents.length}개, 기록 ${cellEntries.length}개. 일정 추가`} onclick={()=>newEvent(day.key)}>
                <span class="paper-date" class:sunday={day.weekday===0} class:saturday={day.weekday===6}>{day.day}{#if day.key===dateKey()}<small>오늘</small>{/if}</span>
                {#each cellEvents.slice(0,2) as e}<span class="paper-event {e.color}" class:done={e.completed}>{e.allDay?'':e.startTime+' '}{e.title}</span>{/each}
                {#if cellEvents.length>2}<small class="cell-more">일정 +{cellEvents.length-2}</small>{/if}{#if cellEntries.length}<small class="cell-record">✎ 기록 {cellEntries.length}</small>{/if}
              </button>
            {/each}
          {/each}
        </div>
        <footer><span>날짜를 누르면 일정을 추가합니다.</span><span>왼쪽 W 선택: 해당 일요일 다음 월요일 ~ 다음 일요일</span></footer>
      </div>
      <section class="selected-day"><div class="section-line"><h2>{readableDate(selected)}</h2><button class="paper-button" onclick={()=>{week=weekContaining(selected);navigate('week');}}>이 날짜의 주간 보기</button><button class="paper-button" onclick={()=>newEntry(selected)}>+ 기록</button></div>{#each events.filter(e=>e.date===selected) as e}{@render eventRow(e)}{:else}<p class="quiet">선택한 날짜에 등록된 일정이 없습니다.</p>{/each}</section>
    {:else if view==='week'}
      <div class="week-paper" aria-label="주간 다이어리 펼침면">
        <section class="week-overview" aria-label="이번 주">
          <small>WEEKLY</small>
          <h2>{parseDate(week.from).getMonth()+1}월</h2>
          <p>{week.from.slice(5).replace('-','.')} — {week.to.slice(5).replace('-','.')}</p>
          <p class="week-help">제목을 눌러 상세 보기<br/>날짜 칸 안에서 스크롤</p>
          {#if more}<button class="paper-button" onclick={loadMore} disabled={busy}>기록 더 불러오기 ({entries.length}개)</button>{/if}
        </section>
        {#each weekDays as date}
          <section class="week-day" class:week-today={date===dateKey()} aria-label={readableDate(date)}>
            <header class:sunday={parseDate(date).getDay()===0} class:saturday={parseDate(date).getDay()===6}>
              <strong>{parseDate(date).getDate()}</strong><small>{parseDate(date).getMonth()+1}월</small>
              {#if date===dateKey()}<em>오늘</em>{/if}<span>{weekdayNames[parseDate(date).getDay()]}요일</span>
            </header>
            <div class="week-day-actions"><button aria-label={`${readableDate(date)} 일정 추가`} onclick={()=>newEvent(date)}>+ 일정</button><button aria-label={`${readableDate(date)} 기록 추가`} onclick={()=>newEntry(date)}>+ 기록</button></div>
            <!-- Keyboard focus lets users scroll this independent day with arrow/PageDown keys. -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <div class="week-day-content" role="region" aria-label={`${readableDate(date)} 기록 목록`} tabindex="0">
              {#each events.filter(e=>e.date===date) as e}{@render eventRow(e)}{/each}
              {#each entries.filter(e=>e.date===date) as entry (entry.id+entry.updatedAt)}<EntryCard {entry} projectTitle={projects.find(p=>p.id===entry.projectId)?.title} onedit={editEntry} />{/each}
              {#each tasks.filter(t=>t.date===date) as task}{@render taskRow(task)}{/each}
              {#if !events.some(e=>e.date===date)&&!entries.some(e=>e.date===date)&&!tasks.some(t=>t.date===date)}<p class="blank-day">오늘의 작은 기록을 남겨 보세요.</p>{/if}
            </div>
          </section>
        {/each}
      </div>
    {:else if view==='projects'}
      <div class="project-picker"><button class:chosen={!projectId} onclick={()=>chooseProject(null)}>모든 프로젝트</button>{#each projects as p}<button class:chosen={projectId===p.id} onclick={()=>chooseProject(p.id)}>{p.title}</button>{/each}</div>
      {#if activeProject}<section class="project-overview"><p class="preserve">{activeProject.description||'프로젝트 설명을 추가해 보세요.'}</p><small>Git 저장소: {activeProject.repoPath||'아직 연결하지 않았습니다.'}</small><div class="diary-actions"><button class="paper-button" onclick={importGit} disabled={busy||!isNative}>{busy?'처리 중…':'Git 커밋 수집'}</button><button class="paper-button" onclick={()=>newEntry(selected,'note')}>+ 자유 노트</button><button class="paper-button" onclick={()=>newEntry(selected,'troubleshooting')}>+ 트러블슈팅</button><button class="paper-button" onclick={()=>newEntry()}>+ 개발 기록</button><button class="paper-button" onclick={()=>draft()} disabled={busy}>프로젝트 근거 요약</button><button class="paper-button" onclick={()=>draft(true)} disabled={busy}>포트폴리오 초안</button></div><p class="hint">최근 500개 커밋을 읽고 저장소·커밋 ID로 중복을 건너뜁니다. 재수집해도 수정한 내용은 보존됩니다. {!isNative?'Git 수집은 데스크톱 앱에서 실행하세요.':''}</p></section>{:else if !projects.length}<section class="diary-empty"><h2>프로젝트의 과정을 모아 보세요.</h2><p>개발 기록, 자유 노트, 문제 해결과 Git 커밋을 한곳에.</p><button class="paper-button primary" onclick={()=>editProject()}>첫 프로젝트 만들기</button></section>{/if}
      <section class="record-list"><h2>{activeProject?'프로젝트 기록':'모든 기록'}</h2>{#each entries as entry (entry.id+entry.updatedAt)}<div class="dated-entry"><small>{entry.date}</small><EntryCard {entry} projectTitle={projects.find(p=>p.id===entry.projectId)?.title} onedit={editEntry} /></div>{:else}{#if !loading}<p class="quiet">아직 기록이 없습니다. 프로젝트를 선택하고 첫 기록을 작성해 보세요.</p>{/if}{/each}</section>
    {:else if view==='records'}
      <p class="view-hint">모든 날짜의 노트, 개발 기록, 트러블슈팅과 저장한 초안입니다.</p><section class="record-list">{#each entries as entry (entry.id+entry.updatedAt)}<div class="dated-entry"><small>{entry.date}</small><EntryCard {entry} projectTitle={projects.find(p=>p.id===entry.projectId)?.title} onedit={editEntry} /></div>{:else}{#if !loading}<section class="diary-empty"><h2>아직 작성한 기록이 없습니다.</h2><button class="paper-button" onclick={()=>newEntry()}>첫 기록 쓰기</button></section>{/if}{/each}</section>
    {:else if view==='tasks'}
      <section class="task-paper"><p class="view-hint">{tasks.filter(t=>t.completed).length} / {tasks.length}개 완료</p>{#each tasks as task}{@render taskRow(task)}{:else}<p class="quiet">새 할 일을 추가해 보세요.</p>{/each}</section>
    {:else if view==='notes'}
      <p class="view-hint">기존 메모도 그대로 보존됩니다. 제목을 선택하면 본문을 불러옵니다.</p><div class="note-tiles">{#each notes as note}<button onclick={()=>editNote(note.id)}><small>자유 메모</small><h2>{note.title}</h2><span>{note.updatedAt.slice(0,10)} 수정</span></button>{:else}<p class="quiet">자유롭게 생각을 남겨 보세요.</p>{/each}</div>
    {:else}
      <section class="settings-paper"><h2>내 컴퓨터에 보관하는 다이어리</h2><p>데이터 저장 위치</p><code>{appInfo?.storagePath??'확인 중…'}</code><h3>바탕화면 위젯</h3><p>{appInfo?.desktopMessage}</p><div class="diary-actions"><button class="paper-button" onclick={()=>attempt(async()=>{if(appInfo?.widgetVisible&&isNative)await api.hideWidget();else await api.showWidget();appInfo=await api.getAppInfo();})}>{isNative?(appInfo?.widgetVisible?'위젯 숨기기':'위젯 표시'):'위젯 미리보기'}</button><button class="paper-button" disabled={!isNative} onclick={()=>attempt(async()=>{await api.showWidget();appInfo=await api.getAppInfo();})}>위젯 위치 복구</button></div>{#if isNative}<label>불투명도 {appInfo?.widgetOpacity??82}%<input type="range" min="40" max="100" value={appInfo?.widgetOpacity??82} onchange={(e)=>{const opacity=Number(e.currentTarget.value);void attempt(async()=>{await api.setWidgetOpacity(opacity);appInfo=await api.getAppInfo();});}} /></label>{/if}<h3>근거 초안과 외부 연결</h3><p>요약과 포트폴리오는 저장된 원문을 정리한 편집 가능한 초안입니다. AI 분석 결과로 표시하지 않습니다.</p><p>외부 AI, ChatGPT/Space 연결, 모바일과 여러 기기 동기화는 아직 연결되지 않았습니다. 현재 모든 작업은 로컬에서 동작합니다.</p></section>
    {/if}
    {#if loading}<p class="diary-loading" role="status">기록을 불러오는 중…</p>{/if}
    {#if more&&['projects','records','month'].includes(view)}<button class="paper-button load-more" onclick={loadMore} disabled={busy}>기록 더 불러오기 (현재 {entries.length}개)</button>{/if}
    <footer class="diary-footnote"><span>하루의 계획과 경험을, 오래 간직하도록.</span><span>TimeSchedule · {isNative?appInfo?.version??'':'미리보기'}</span></footer>
  </main>
</div>

{#snippet eventRow(event:CalendarEvent)}
  <details class="diary-event" class:done={event.completed}><summary><span class="event-dot {event.color}"></span><small>{event.allDay?'종일':`${event.startTime}–${event.endTime}`}</small><strong>{event.title}</strong></summary><div><p class="preserve">{event.notes||'메모가 없습니다.'}</p><button class="paper-button" onclick={()=>editEvent(event)}>일정 수정</button></div></details>
{/snippet}
{#snippet taskRow(task:Task)}
  <div class="diary-task" class:done={task.completed}><button class="task-tick" aria-label={`${task.title} 완료 ${task.completed?'취소':'표시'}`} aria-pressed={task.completed} onclick={()=>toggleTask(task)}>{task.completed?'✓':''}</button><button class="task-open" onclick={()=>openEditor({type:'task',date:task.date??selected,value:{...task}})}>{task.title}<small>{task.date??'날짜 없음'}</small></button></div>
{/snippet}
{#if editor}{#key editorKey}<DiaryEditor request={editor} {projects} onclose={()=>editor=null} onsaved={saved} />{/key}{/if}
