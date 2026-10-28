<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from './lib/Icon.svelte';
  import { api, isNative, onAppError, onDataChange, onDesktopChange, onSelectDate, validateEvent } from './lib/api';
  import type { AppInfo, CalendarEvent, EventColor, EventInput, Note, NoteInput, Task, TaskInput } from './lib/api';
  import { calendarDays, dateKey, monthStart, parseDate, readableDate, shiftMonth, weekdayNames } from './lib/dates';

  const params = new URLSearchParams(window.location.search);
  const widget = params.get('view') === 'widget';
  const requestedDate = params.get('date');
  const initialToday = dateKey();
  const initialSelected = requestedDate && /^\d{4}-\d{2}-\d{2}$/.test(requestedDate) && dateKey(parseDate(requestedDate)) === requestedDate ? requestedDate : initialToday;
  let today = $state(initialToday);
  let selected = $state(initialSelected);
  let month = $state(monthStart(initialSelected));
  let page = $state<'calendar' | 'tasks' | 'notes'>('calendar');
  let events = $state<CalendarEvent[]>([]);
  let tasks = $state<Task[]>([]);
  let notes = $state<Note[]>([]);
  let appInfo = $state<AppInfo | null>(null);
  let loading = $state(true);
  let error = $state('');
  let nativeError = $state('');
  let busy = $state(false);
  let toast = $state('');
  let widgetActionBusy = $state(false);
  let opacityDraft = $state<number | null>(null);
  let showInfo = $state(false);
  let taskTitle = $state('');
  let taskFilter = $state<'all' | 'open' | 'done'>('all');
  let dialog: HTMLDialogElement;
  let editor = $state<'event' | 'note' | 'task'>('event');
  let form = $state<EventInput>(emptyEvent(initialSelected));
  let noteForm = $state<NoteInput>({ title: '', body: '' });
  let taskForm = $state<TaskInput>({ title: '', date: null, completed: false });
  let formError = $state('');
  let deleting = $state(false);
  let loadNumber = 0;
  let infoLoadNumber = 0;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  const colors: { value: EventColor; label: string }[] = [
    { value: 'sage', label: '세이지' }, { value: 'blue', label: '블루' },
    { value: 'amber', label: '앰버' }, { value: 'rose', label: '로즈' },
  ];
  const days = $derived(calendarDays(month));
  const monthLabel = $derived(`${month.getFullYear()}년 ${month.getMonth() + 1}월`);
  const dayEvents = $derived(events.filter(event => event.date === selected));
  const todayEvents = $derived(events.filter(event => event.date === today));
  const monthEvents = $derived(events.filter(event => event.date.startsWith(dateKey(month).slice(0, 7))));
  const selectedTasks = $derived(tasks.filter(task => !task.date || task.date === selected));
  const openTasks = $derived(tasks.filter(task => !task.completed));
  const filteredTasks = $derived(tasks.filter(task => taskFilter === 'all' || (taskFilter === 'done' ? task.completed : !task.completed)));
  const sortedNotes = $derived([...notes].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)));
  const editingId = $derived(editor === 'event' ? form.id : editor === 'note' ? noteForm.id : taskForm.id);
  const locked = $derived(appInfo?.widgetLocked ?? true);
  const visibleError = $derived(nativeError || error);
  const widgetOpacity = $derived(opacityDraft ?? appInfo?.widgetOpacity ?? 82);

  function emptyEvent(date: string): EventInput {
    return { title: '', date, startTime: null, endTime: null, allDay: true, notes: '', color: 'sage', completed: false };
  }

  function message(cause: unknown): string {
    return cause instanceof Error ? cause.message : String(cause);
  }

  async function refresh(): Promise<void> {
    const version = ++loadNumber;
    const grid = calendarDays(month);
    const boundaries = [grid[0].key, grid[41].key, today, selected].sort();
    const from = boundaries[0];
    const to = boundaries[3];
    try {
      const [nextEvents, nextTasks, nextNotes] = await Promise.all([api.listEvents(from, to), api.listTasks(), api.listNotes()]);
      if (version !== loadNumber) return;
      events = nextEvents;
      tasks = nextTasks;
      notes = nextNotes;
      if (error.startsWith('데이터를 불러오지 못했어요.')) error = '';
    } catch (cause) {
      if (version === loadNumber) error = `데이터를 불러오지 못했어요. ${message(cause)}`;
    } finally { if (version === loadNumber) loading = false; }
  }

  async function refreshAppInfo(): Promise<void> {
    const version = ++infoLoadNumber;
    const next = await api.getAppInfo();
    if (version === infoLoadNumber) appInfo = next;
  }

  async function retryConnection(): Promise<void> {
    nativeError = '';
    error = '';
    await perform(async () => { await Promise.all([refresh(), refreshAppInfo()]); });
  }

  function notify(text: string): void {
    toast = text;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toast = ''; }, 2800);
  }

  async function perform(action: () => Promise<unknown>, success?: string): Promise<void> {
    try { await action(); if (success) notify(success); }
    catch (cause) { error = message(cause); }
  }

  function changeMonth(delta: number): void { month = shiftMonth(month, delta); void refresh(); }

  function selectDate(date: string): void {
    selected = date;
    page = 'calendar';
    if (date.slice(0, 7) !== dateKey(month).slice(0, 7)) { month = monthStart(date); void refresh(); }
  }

  function goToday(): void { selectDate(today); }

  function clickDate(date: string): void {
    if (widget) void perform(() => api.showManager(date));
    else selectDate(date);
  }

  async function editEvent(event?: CalendarEvent, date = selected): Promise<void> {
    editor = 'event';
    form = event ? { ...event } : emptyEvent(date);
    formError = '';
    deleting = false;
    await tick();
    dialog.showModal();
    dialog.querySelector<HTMLInputElement>('input')?.focus();
  }

  async function editNote(note?: Note): Promise<void> {
    editor = 'note';
    noteForm = note ? { ...note } : { title: '', body: '' };
    formError = '';
    deleting = false;
    await tick();
    dialog.showModal();
    dialog.querySelector<HTMLInputElement>('input')?.focus();
  }

  async function editTask(task: Task): Promise<void> {
    editor = 'task';
    taskForm = { ...task };
    formError = '';
    deleting = false;
    await tick();
    dialog.showModal();
    dialog.querySelector<HTMLInputElement>('input')?.focus();
  }

  function closeDialog(): void { if (!busy) dialog.close(); }

  async function saveEditor(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (busy) return;
    formError = editor === 'event' ? validateEvent(form) ?? '' : '';
    if (formError) return;
    busy = true;
    try {
      if (editor === 'event') {
        const input = { ...form };
        await api.saveEvent(input);
        selectDate(input.date);
      }
      else if (editor === 'note') await api.saveNote(noteForm);
      else await api.saveTask(taskForm);
      dialog.close();
      await refresh();
      notify('저장했어요.');
    } catch (cause) { formError = message(cause); }
    finally { busy = false; }
  }

  async function deleteEditor(): Promise<void> {
    if (!editingId || busy) return;
    if (!deleting) { deleting = true; return; }
    busy = true;
    try {
      if (editor === 'event') await api.deleteEvent(editingId);
      else if (editor === 'note') await api.deleteNote(editingId);
      else await api.deleteTask(editingId);
      dialog.close();
      await refresh();
      notify('삭제했어요.');
    } catch (cause) { formError = message(cause); }
    finally { busy = false; }
  }

  async function addTask(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!taskTitle.trim() || busy) return;
    busy = true;
    try { await api.saveTask({ title: taskTitle, date: page === 'tasks' ? null : selected, completed: false }); taskTitle = ''; await refresh(); }
    catch (cause) { error = message(cause); }
    finally { busy = false; }
  }

  async function toggleTask(task: Task): Promise<void> {
    await perform(async () => { await api.saveTask({ ...task, completed: !task.completed }); await refresh(); });
  }

  async function toggleEvent(event: CalendarEvent): Promise<void> {
    await perform(async () => { await api.saveEvent({ ...event, completed: !event.completed }); await refresh(); });
  }

  async function toggleLock(): Promise<void> {
    if (widgetActionBusy || !appInfo) return;
    widgetActionBusy = true;
    await perform(async () => { await api.setWidgetLocked(!locked); await refreshAppInfo(); });
    widgetActionBusy = false;
  }

  async function toggleWidget(): Promise<void> {
    if (widgetActionBusy) return;
    widgetActionBusy = true;
    const hide = isNative && appInfo?.widgetVisible;
    await perform(async () => {
      if (hide) await api.hideWidget(); else await api.showWidget();
      await refreshAppInfo();
    }, isNative ? hide ? '바탕화면 위젯을 숨겼어요.' : '바탕화면 위젯을 표시했어요.' : undefined);
    widgetActionBusy = false;
  }

  async function commitOpacity(event: Event): Promise<void> {
    if (widgetActionBusy) return;
    const opacity = Number((event.currentTarget as HTMLInputElement).value);
    widgetActionBusy = true;
    await perform(async () => { await api.setWidgetOpacity(opacity); await refreshAppInfo(); });
    opacityDraft = null;
    widgetActionBusy = false;
  }

  async function recoverWidget(): Promise<void> {
    if (widgetActionBusy) return;
    widgetActionBusy = true;
    await perform(async () => {
      await api.showWidget();
      await refreshAppInfo();
    }, '바탕화면 위젯을 화면 안으로 복구했어요.');
    widgetActionBusy = false;
  }

  function timeLabel(event: CalendarEvent): string {
    return event.allDay ? '하루 종일' : `${event.startTime} – ${event.endTime}`;
  }

  onMount(() => {
    const disposers: (() => void)[] = [];
    let disposed = false;
    let midnightTimer: ReturnType<typeof setTimeout>;
    function updateToday(): void {
      const nextToday = dateKey();
      if (nextToday !== today) {
        const previous = today;
        today = nextToday;
        if (selected === previous) { selected = nextToday; month = monthStart(nextToday); }
        if (widget) month = monthStart(nextToday);
        void refresh();
      }
    }
    function scheduleMidnight(): void {
      updateToday();
      const now = new Date();
      const next = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1, 0, 0, 1);
      midnightTimer = setTimeout(scheduleMidnight, next.getTime() - now.getTime());
    }
    function onWake(): void { updateToday(); }
    void (async () => {
      try {
        for (const subscribe of [
          () => onDataChange(() => { if (!disposed) void refresh(); }),
          () => onSelectDate(date => { if (!disposed && !widget) selectDate(date); }),
          () => onDesktopChange(() => { if (!disposed) void perform(refreshAppInfo); }),
          () => onAppError(text => { if (!disposed) nativeError = text; }),
        ]) {
          if (disposed) return;
          const dispose = await subscribe();
          if (disposed) { dispose(); return; }
          disposers.push(dispose);
        }
      } catch (cause) {
        if (!disposed) error = `앱 연결을 확인해 주세요. ${message(cause)}`;
      } finally {
        if (!disposed) {
          void refresh();
          void perform(refreshAppInfo);
        }
      }
    })();
    scheduleMidnight();
    window.addEventListener('focus', onWake);
    document.addEventListener('visibilitychange', onWake);
    return () => {
      disposed = true;
      disposers.forEach(dispose => dispose());
      clearTimeout(midnightTimer);
      clearTimeout(toastTimer);
      window.removeEventListener('focus', onWake);
      document.removeEventListener('visibilitychange', onWake);
    };
  });
</script>

{#snippet agendaItem(event: CalendarEvent, compact = false)}
  <div class:completed={event.completed} class="agenda-item {event.color}">
    <span class="event-line"></span>
    <button class="agenda-detail" onclick={() => compact ? perform(() => api.showManager(event.date)) : editEvent(event)}>
      <span class="event-time">{timeLabel(event)}</span>
      <span class="event-title">{event.title}</span>
      {#if event.notes && !compact}<span class="event-note">{event.notes}</span>{/if}
    </button>
    {#if !compact}<button class="round-check" class:checked={event.completed} aria-label={`${event.title} ${event.completed ? '완료 취소' : '완료'}`} aria-pressed={event.completed} onclick={() => toggleEvent(event)}>{#if event.completed}<Icon name="check" size={12} />{/if}</button>{/if}
  </div>
{/snippet}

{#snippet taskItem(task: Task, full = false)}
  <div class="task-row" class:completed={task.completed}>
    <button class="task-checkbox" class:checked={task.completed} aria-label={`${task.title} ${task.completed ? '완료 취소' : '완료'}`} aria-pressed={task.completed} onclick={() => toggleTask(task)}>{#if task.completed}<Icon name="check" size={12} />{/if}</button>
    <button class="task-label" onclick={() => editTask(task)}><span>{task.title}</span>{#if full && task.date}<small>{readableDate(task.date)}</small>{/if}</button>
    {#if full}<button class="icon-button subtle" aria-label={`${task.title} 수정`} onclick={() => editTask(task)}><Icon name="right" size={17} /></button>{/if}
  </div>
{/snippet}

{#if widget}
  <main class="desktop-calendar" class:move-enabled={isNative && !locked} style:opacity={isNative ? 1 : widgetOpacity / 100}>
    <header class="desktop-toolbar" data-tauri-drag-region={isNative && !locked ? 'deep' : 'false'}>
      <div class="desktop-month-heading">
        <Icon name="calendar" size={21} />
        <h1>{monthLabel}</h1>
        <div class="desktop-month-nav" data-tauri-drag-region="false">
          <button class="icon-button" aria-label="이전 달" onclick={() => changeMonth(-1)}><Icon name="left" size={18} /></button>
          <button class="desktop-today-button" onclick={() => { month = monthStart(today); void refresh(); }}>오늘</button>
          <button class="icon-button" aria-label="다음 달" onclick={() => changeMonth(1)}><Icon name="right" size={18} /></button>
        </div>
      </div>
      <div class="desktop-toolbar-actions" data-tauri-drag-region="false">
        <button class="desktop-move-button" class:active={!locked} aria-pressed={!locked} title={!isNative ? '위젯 이동은 데스크톱 앱에서 사용할 수 있어요.' : undefined} disabled={!isNative || widgetActionBusy || !appInfo} onclick={toggleLock}>
          <Icon name={locked ? 'move' : 'lock'} size={15} />{locked ? '이동하기' : '위치 고정'}
        </button>
        <button class="desktop-manage-button" onclick={() => perform(() => api.showManager(today))}><span>일정 관리</span><Icon name="arrow" size={15} /></button>
      </div>
    </header>
    <div class="desktop-move-hint" data-tauri-drag-region={isNative && !locked ? 'deep' : 'false'}>
      <Icon name={locked ? 'lock' : 'grip'} size={13} />
      <span>{!isNative ? '브라우저 미리보기 · 불투명도는 CSS 시뮬레이션이며, 실제 바탕화면 모습과 다를 수 있어요.' : locked ? '위치가 고정되어 있어요. 옮기려면 오른쪽 위의 ‘이동하기’를 누르세요.' : '이 안내 영역이나 상단 빈 공간을 드래그해 옮긴 뒤 ‘위치 고정’을 누르세요.'}</span>
    </div>
    <section class="desktop-month" aria-label={`${monthLabel} 바탕화면 달력`}>
      <div class="desktop-weekdays">{#each weekdayNames as name, index}<span class:sunday={index === 0} class:saturday={index === 6}>{name}</span>{/each}</div>
      <div class="desktop-day-grid">
        {#each days as day}
          {@const cellEvents = events.filter(event => event.date === day.key)}
          <button class="desktop-day" class:outside={!day.currentMonth} class:is-today={day.key === today} aria-label={`${readableDate(day.key)}${day.key === today ? ', 오늘' : ''}, 일정 ${cellEvents.length}개. 일정 관리 열기`} aria-current={day.key === today ? 'date' : undefined} onclick={() => clickDate(day.key)}>
            <span class="desktop-day-heading"><span class="desktop-day-number" class:sunday={day.weekday === 0} class:saturday={day.weekday === 6}>{day.day}</span>{#if day.key === today}<span class="desktop-today-label">오늘</span>{/if}{#if cellEvents.length > 2}<span class="desktop-more">+{cellEvents.length - 2}</span>{/if}</span>
            <span class="desktop-day-events">
              {#each cellEvents.slice(0, 2) as event}
                <span class="desktop-event {event.color}" class:completed={event.completed}><i></i>{#if !event.allDay}<span class="desktop-event-time">{event.startTime}</span>{/if}<span class="desktop-event-title">{event.title}</span></span>
              {/each}
            </span>
          </button>
        {/each}
      </div>
    </section>
    <section class="desktop-today-strip" aria-label="오늘의 일정">
      <button class="desktop-today-summary" onclick={() => perform(() => api.showManager(today))}><span class="desktop-today-pill">오늘</span><strong>{parseDate(today).getMonth() + 1}.{parseDate(today).getDate()}</strong><span>{weekdayNames[parseDate(today).getDay()]}요일</span></button>
      {#if loading}<span class="desktop-empty">일정을 불러오는 중이에요…</span>{:else if todayEvents.length}<div class="desktop-today-events">{#each todayEvents.slice(0, 3) as event}<button class="desktop-today-event {event.color}" class:completed={event.completed} onclick={() => perform(() => api.showManager(today))}><i></i><span class="desktop-event-time">{event.allDay ? '종일' : event.startTime}</span><span>{event.title}</span></button>{/each}</div>{#if todayEvents.length > 3}<button class="desktop-today-more" onclick={() => perform(() => api.showManager(today))}>+{todayEvents.length - 3}개</button>{/if}{:else}<span class="desktop-empty">오늘 등록된 일정이 없어요.</span>{/if}
      <button class="desktop-add-button" aria-label="오늘 일정 관리 열기" onclick={() => perform(() => api.showManager(today))}><Icon name="plus" size={16} /></button>
    </section>
    {#if visibleError}<div class="desktop-error" role="alert"><span>{visibleError}</span><button onclick={retryConnection}>다시 시도</button></div>{/if}
    <footer class="desktop-footer">
      <span><i class="status-dot" class:warning={!appInfo?.desktopAttached}></i><span title={appInfo?.desktopMessage}>{!isNative ? '브라우저 미리보기 · 브라우저에 별도 저장' : appInfo?.desktopAttached ? '바탕화면에 연결됨' : '바탕화면 연결 확인 필요'}</span></span>
      <span>날짜를 누르면 일정을 관리할 수 있어요.</span>
      <label class="desktop-opacity" title="값을 낮출수록 바탕화면이 더 많이 비쳐 보여요."><span>불투명도</span><input type="range" min="40" max="100" step="1" value={widgetOpacity} disabled={widgetActionBusy || !appInfo} oninput={(event) => opacityDraft = Number(event.currentTarget.value)} onchange={commitOpacity} /><output>{widgetOpacity}%</output></label>
      <span class="desktop-signature">TimeSchedule</span>
    </footer>
  </main>
{:else}
  <div class="app-shell">
    <aside class="sidebar">
      <a class="brand" href="#calendar" onclick={(event) => { event.preventDefault(); page = 'calendar'; }}><span class="brand-mark"><Icon name="calendar" size={22} /></span><span>TimeSchedule<small>나를 위한 작은 시간표</small></span></a>
      <p class="nav-label">MY SPACE</p>
      <nav aria-label="주 메뉴">
        <button class:active={page === 'calendar'} onclick={() => page = 'calendar'}><Icon name="calendar" size={19} /><span>캘린더</span>{#if page === 'calendar'}<i></i>{/if}</button>
        <button class:active={page === 'tasks'} onclick={() => page = 'tasks'}><Icon name="tasks" size={19} /><span>할 일</span>{#if openTasks.length}<span class="nav-count">{openTasks.length}</span>{/if}</button>
        <button class:active={page === 'notes'} onclick={() => page = 'notes'}><Icon name="note" size={19} /><span>메모</span></button>
      </nav>
      <div class="sidebar-today"><div class="sidebar-today-label"><Icon name="sun" size={16} />오늘, {parseDate(today).getMonth() + 1}월 {parseDate(today).getDate()}일</div><p>작은 계획으로<br />가벼워지는 하루.</p><button onclick={goToday}>오늘 일정 <strong>{todayEvents.length}개</strong><Icon name="arrow" size={15} /></button></div>
      <div class="sidebar-bottom"><button class="widget-launch" onclick={toggleWidget} disabled={widgetActionBusy || (isNative && !appInfo)} aria-label={isNative ? appInfo?.widgetVisible ? '바탕화면 위젯 숨기기' : '바탕화면 위젯 표시' : '위젯 미리보기'} aria-pressed={isNative ? appInfo?.widgetVisible ?? false : undefined}><Icon name="monitor" size={18} /><span>{isNative ? appInfo?.widgetVisible ? '위젯 숨기기' : '바탕화면 위젯' : '위젯 미리보기'}</span><Icon name={isNative && appInfo?.widgetVisible ? 'check' : 'arrow'} size={14} /></button>{#if isNative}<button class="widget-recover" onclick={recoverWidget} disabled={widgetActionBusy} title="화면 밖으로 나간 위젯의 위치와 크기를 복구합니다" aria-label="위젯 위치 복구"><Icon name="refresh" size={15} /><span>위젯 위치 복구</span></button>{/if}<button class="storage-status" onclick={() => showInfo = !showInfo} aria-expanded={showInfo}><span class="status-dot"></span>{isNative ? '내 컴퓨터에 저장' : '브라우저 미리보기'}<span class="info-letter">i</span></button>{#if showInfo}<div class="storage-info"><strong>{isNative ? '오프라인 · SQLite' : '미리보기 · localStorage'}</strong><p>{appInfo?.storagePath ?? '저장 위치 확인 중…'}</p><p>{appInfo?.desktopMessage}</p></div>{/if}<div class="sidebar-version">PERSONAL CALENDAR <span>v{isNative ? appInfo?.version ?? '0.1.0' : '0.1'}</span></div></div>
    </aside>

    <main class="main-content">
      <header class="page-header"><div><p class="eyebrow">{page === 'calendar' ? 'A LITTLE SPACE FOR YOUR DAY' : page === 'tasks' ? 'ONE THING AT A TIME' : 'KEEP A LITTLE THOUGHT'}</p><h1>{page === 'calendar' ? '내 하루를, 차분하게.' : page === 'tasks' ? '하나씩, 가볍게.' : '잊고 싶지 않은 생각.'}</h1></div><div class="header-date"><span>{parseDate(today).getFullYear()}</span>{parseDate(today).getMonth() + 1}월 {parseDate(today).getDate()}일 {weekdayNames[parseDate(today).getDay()]}요일</div></header>
      {#if !isNative}<div class="preview-banner"><span>브라우저 미리보기</span>이 브라우저에만 저장됩니다. 데스크톱 앱 데이터와는 별개예요.</div>{/if}
      {#if visibleError}<div class="error-banner" role="alert"><span>{visibleError}</span><button onclick={retryConnection} class="text-button"><Icon name="refresh" size={15} /> 다시 시도</button><button class="icon-button" aria-label="오류 알림 닫기" onclick={() => { error = ''; nativeError = ''; }}><Icon name="close" size={16} /></button></div>{/if}

      {#if page === 'calendar'}
        <div class="calendar-layout">
          <section class="calendar-panel" aria-label="월간 캘린더">
            <div class="calendar-toolbar"><div class="month-heading"><h2>{monthLabel}</h2><div class="month-arrows"><button class="icon-button" aria-label="이전 달" onclick={() => changeMonth(-1)}><Icon name="left" size={18} /></button><button class="icon-button" aria-label="다음 달" onclick={() => changeMonth(1)}><Icon name="right" size={18} /></button></div><button class="today-button" onclick={goToday}>오늘</button></div><button class="primary-button" onclick={() => editEvent()}><Icon name="plus" size={17} /><span>일정 추가</span></button></div>
            <div class="calendar-weekdays">{#each weekdayNames as name, index}<span class:sunday={index === 0} class:saturday={index === 6}>{name}</span>{/each}</div>
            <div class="calendar-grid" aria-label={monthLabel}>
              {#each days as day}
                {@const cellEvents = events.filter(event => event.date === day.key)}
                <button class="calendar-cell" class:outside={!day.currentMonth} class:selected={selected === day.key} class:today-cell={day.key === today} aria-label={`${readableDate(day.key)}${day.key === today ? ', 오늘' : ''}, 일정 ${cellEvents.length}개`} aria-pressed={selected === day.key} aria-current={day.key === today ? 'date' : undefined} onclick={() => clickDate(day.key)} ondblclick={() => editEvent(undefined, day.key)}>
                  <span class="cell-date" class:sunday={day.weekday === 0} class:saturday={day.weekday === 6}><span class="day-number">{day.day}</span>{#if day.key === today}<span class="today-word">오늘</span>{/if}</span>
                  <span class="cell-events">{#each cellEvents.slice(0, 2) as event}<span class="calendar-event {event.color}" class:completed={event.completed}><i></i><span>{event.title}</span></span>{/each}{#if cellEvents.length > 2}<span class="more-events">+{cellEvents.length - 2}개 더</span>{/if}</span>
                </button>
              {/each}
            </div>
            <footer class="calendar-footer"><span><i class="legend-today"></i>오늘<i class="legend-selected"></i>선택한 날짜</span><span>이번 달 일정 <strong>{monthEvents.length}개</strong></span></footer>
          </section>

          <aside class="day-panel" aria-label="선택한 날짜의 일정">
            <div class="day-panel-heading"><p>{selected === today ? 'TODAY’S PLAN' : 'YOUR DAILY PLAN'}</p><div><h2>{parseDate(selected).getMonth() + 1}월 {parseDate(selected).getDate()}일 <span>{weekdayNames[parseDate(selected).getDay()]}요일</span></h2>{#if selected === today}<span class="today-tag">오늘</span>{/if}</div></div>
            <div class="day-agenda"><div class="section-title"><h3>일정 <span class="count">{dayEvents.length}</span></h3><button class="icon-button" aria-label="선택한 날짜에 일정 추가" onclick={() => editEvent()}><Icon name="plus" size={17} /></button></div>
              {#if loading}<p class="loading-text">일정을 불러오는 중이에요…</p>{:else if dayEvents.length}<div class="agenda-list">{#each dayEvents as event}{@render agendaItem(event)}{/each}</div>{:else}<div class="empty-agenda"><div class="empty-leaf"><Icon name="leaf" size={30} /></div><h3>여유로운 하루예요</h3><p>아직 등록된 일정이 없어요.<br />작은 계획부터 적어 볼까요?</p><button class="text-button" onclick={() => editEvent()}><Icon name="plus" size={14} /> 첫 일정 추가하기</button></div>{/if}
            </div>
            <div class="day-tasks"><div class="section-title"><h3>할 일 <span class="count">{selectedTasks.filter(task => !task.completed).length}</span></h3><button class="text-button" onclick={() => page = 'tasks'}>모두 보기 <Icon name="right" size={12} /></button></div>{#each selectedTasks.slice(0, 4) as task}{@render taskItem(task)}{/each}<form class="inline-task-form" onsubmit={addTask}><Icon name="plus" size={16} /><input aria-label="선택한 날짜에 할 일 추가" placeholder="작은 할 일 추가" bind:value={taskTitle} maxlength="200" disabled={busy} /><button type="submit" class="icon-button" aria-label="할 일 저장" disabled={!taskTitle.trim() || busy}><Icon name="arrow" size={15} /></button></form></div>
            <p class="day-panel-note">오늘의 속도는 내가 정하는 거니까.</p>
          </aside>
        </div>
        <div class="bottom-caption"><span>하루를 채우기보다, 소중한 시간을 기억해요.</span><span>TimeSchedule</span></div>
      {:else if page === 'tasks'}
        <section class="collection-panel"><div class="collection-heading"><div><h2>나의 할 일 <span class="count">{tasks.length}</span></h2><p>해야 할 일도, 하고 싶은 일도 한곳에.</p></div><span class="completion-count"><strong>{tasks.filter(task => task.completed).length}</strong> / {tasks.length} 완료</span></div><form class="task-add-form" onsubmit={addTask}><Icon name="plus" size={19} /><input aria-label="새 할 일" placeholder="어떤 일을 해 볼까요?" bind:value={taskTitle} maxlength="200" disabled={busy} /><button type="submit" class="primary-button" disabled={!taskTitle.trim() || busy}>추가</button></form><div class="filter-tabs" aria-label="할 일 필터">{#each [{ value: 'all', label: '전체' }, { value: 'open', label: '진행 중' }, { value: 'done', label: '완료' }] as filter}<button class:active={taskFilter === filter.value} onclick={() => taskFilter = filter.value as typeof taskFilter}>{filter.label}</button>{/each}</div><div class="full-task-list">{#each filteredTasks as task}{@render taskItem(task, true)}{:else}<div class="collection-empty"><Icon name="tasks" size={36} /><h3>{taskFilter === 'done' ? '완료한 일이 여기에 모여요' : '작은 한 걸음부터 시작해요'}</h3><p>{taskFilter === 'done' ? '할 일을 마치면 체크해 보세요.' : '위에 할 일을 적고 추가해 주세요.'}</p></div>{/each}</div></section>
      {:else}
        <section class="notes-section"><div class="collection-heading"><div><h2>나의 메모 <span class="count">{notes.length}</span></h2><p>스쳐 가는 생각을 잠시 붙잡아 두세요.</p></div><button class="primary-button" onclick={() => editNote()}><Icon name="plus" size={17} />메모 추가</button></div>{#if notes.length}<div class="notes-grid">{#each sortedNotes as note}<button class="note-card" onclick={() => editNote(note)}><Icon name="note" size={19} /><h3>{note.title}</h3><p>{note.body || '내용이 없는 메모예요.'}</p><small>{new Date(note.updatedAt).toLocaleDateString('ko-KR', { month: 'long', day: 'numeric' })} 수정</small></button>{/each}</div>{:else}<div class="collection-empty notes-empty"><span class="empty-leaf"><Icon name="note" size={31} /></span><h3>생각이 머무는 작은 공간</h3><p>아이디어, 기억할 것, 오늘의 한 줄.<br />무엇이든 편하게 남겨 보세요.</p><button class="text-button" onclick={() => editNote()}><Icon name="plus" size={15} />첫 메모 쓰기</button></div>{/if}</section>
      {/if}
    </main>
  </div>
{/if}

<dialog bind:this={dialog} class="editor-dialog" aria-labelledby="editor-title" oncancel={(event) => { if (busy) event.preventDefault(); }}>
  <form onsubmit={saveEditor}>
    <header class="dialog-header"><div><p class="eyebrow">{editor === 'event' ? 'A MOMENT TO REMEMBER' : editor === 'note' ? 'A LITTLE THOUGHT' : 'ONE SMALL STEP'}</p><h2 id="editor-title">{editor === 'event' ? `일정 ${editingId ? '수정' : '추가'}` : editor === 'note' ? `메모 ${editingId ? '수정' : '추가'}` : '할 일 수정'}</h2></div><button type="button" class="icon-button" aria-label="편집 창 닫기" onclick={closeDialog} disabled={busy}><Icon name="close" size={20} /></button></header>
    <fieldset class="dialog-body" disabled={busy}>
      {#if editor === 'event'}
        <label class="field">제목<input required maxlength="200" placeholder="어떤 일정인가요?" bind:value={form.title} /></label>
        <label class="field">날짜<input type="date" required min="1900-01-01" max="9999-12-31" bind:value={form.date} /></label>
        <label class="checkbox-label"><input type="checkbox" bind:checked={form.allDay} onchange={(event) => { if (!event.currentTarget.checked) { form.startTime ||= '09:00'; form.endTime ||= '10:00'; } }} />하루 종일</label>
        {#if !form.allDay}<div class="time-fields"><label class="field">시작 시간<input type="time" required bind:value={form.startTime} /></label><span>–</span><label class="field">종료 시간<input type="time" required bind:value={form.endTime} /></label></div>{/if}
        <fieldset class="color-field"><legend>일정 색상</legend><div>{#each colors as color}<label class="color-option {color.value}" class:chosen={form.color === color.value}><input type="radio" name="color" value={color.value} bind:group={form.color} aria-label={color.label} />{#if form.color === color.value}<Icon name="check" size={16} />{/if}</label>{/each}</div></fieldset>
        <label class="field">메모 <span class="optional">선택</span><textarea rows="3" maxlength="20000" placeholder="기억해 둘 내용을 적어 주세요." bind:value={form.notes}></textarea></label>
        {#if editingId}<label class="checkbox-label"><input type="checkbox" bind:checked={form.completed} />완료한 일정</label>{/if}
      {:else if editor === 'note'}
        <label class="field">제목<input required maxlength="200" placeholder="메모 제목" bind:value={noteForm.title} /></label><label class="field">내용<textarea rows="9" maxlength="20000" placeholder="떠오르는 생각을 적어 보세요." bind:value={noteForm.body}></textarea></label>
      {:else}
        <label class="field">할 일<input required maxlength="200" bind:value={taskForm.title} /></label><label class="field">날짜 <span class="optional">선택</span><input type="date" min="1900-01-01" max="9999-12-31" value={taskForm.date ?? ''} oninput={(event) => taskForm.date = event.currentTarget.value || null} /></label><label class="checkbox-label"><input type="checkbox" bind:checked={taskForm.completed} />완료한 일</label>
      {/if}
      {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
      {#if deleting}<p class="delete-confirm" role="alert">삭제하면 되돌릴 수 없어요. 삭제할까요?</p>{/if}
    </fieldset>
    <footer class="dialog-footer">{#if editingId}<button type="button" class="delete-button" class:confirming={deleting} onclick={deleteEditor} disabled={busy}><Icon name="trash" size={16} />{deleting ? '삭제 확인' : '삭제'}</button>{/if}<div><button type="button" class="secondary-button" onclick={() => { if (deleting) deleting = false; else closeDialog(); }} disabled={busy}>취소</button><button type="submit" class="primary-button" disabled={busy}>{busy ? '저장 중…' : '저장하기'}</button></div></footer>
  </form>
</dialog>
{#if toast}<div class="toast" role="status"><Icon name="check" size={17} />{toast}</div>{/if}
