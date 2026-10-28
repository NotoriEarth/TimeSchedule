import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type EventColor = 'sage' | 'blue' | 'amber' | 'rose';
export type CalendarEvent = {
  id: string; title: string; date: string; startTime: string | null;
  endTime: string | null; allDay: boolean; notes: string; color: EventColor;
  completed: boolean; updatedAt: string;
};
export type EventInput = Omit<CalendarEvent, 'id' | 'updatedAt'> & { id?: string };
export type Task = { id: string; title: string; date: string | null; completed: boolean; updatedAt: string };
export type TaskInput = Omit<Task, 'id' | 'updatedAt'> & { id?: string };
export type Note = { id: string; title: string; body: string; updatedAt: string };
export type NoteInput = Omit<Note, 'id' | 'updatedAt'> & { id?: string };
export type AppInfo = { storagePath: string; version: string; desktopAttached: boolean; desktopMessage: string; widgetLocked: boolean; widgetVisible: boolean; widgetOpacity: number };

type PreviewDatabase = { events: CalendarEvent[]; tasks: Task[]; notes: Note[] };
export const isNative = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const storageKey = 'timeschedule-preview-v1';
const changeEvent = 'timeschedule-local-change';
let previewWidgetOpacity = 82;

function readPreview(): PreviewDatabase {
  const raw = localStorage.getItem(storageKey);
  if (!raw) return { events: [], tasks: [], notes: [] };
  const data: unknown = JSON.parse(raw);
  if (!data || typeof data !== 'object' || !('events' in data) || !Array.isArray(data.events) || !('tasks' in data) || !Array.isArray(data.tasks) || !('notes' in data) || !Array.isArray(data.notes)) {
    throw new Error('브라우저 미리보기 데이터를 읽을 수 없습니다. 브라우저 저장소를 확인해 주세요.');
  }
  return data as PreviewDatabase;
}

function writePreview(data: PreviewDatabase): void {
  localStorage.setItem(storageKey, JSON.stringify(data));
  window.dispatchEvent(new Event(changeEvent));
}

function validateDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(year, month - 1, day, 12);
  return year >= 1900 && year <= 9999 && date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day;
}

export function validateEvent(input: EventInput): string | null {
  if (!input.title.trim()) return '일정 제목을 입력해 주세요.';
  if ([...input.title.trim()].length > 200) return '일정 제목은 200자까지 입력할 수 있어요.';
  if ([...input.notes].length > 20000) return '메모는 20,000자까지 입력할 수 있어요.';
  if (!['sage', 'blue', 'amber', 'rose'].includes(input.color)) return '올바른 일정 색상을 선택해 주세요.';
  if (!validateDate(input.date)) return '올바른 날짜를 선택해 주세요. (1900–9999년)';
  if (!input.allDay) {
    const timePattern = /^([01]\d|2[0-3]):[0-5]\d$/;
    if (!input.startTime || !input.endTime || !timePattern.test(input.startTime) || !timePattern.test(input.endTime)) return '시작 시간과 종료 시간을 입력해 주세요.';
    if (input.endTime <= input.startTime) return '종료 시간은 시작 시간보다 늦어야 해요.';
  }
  return null;
}

function sortEvents(events: CalendarEvent[]): CalendarEvent[] {
  return events.sort((a, b) => a.date.localeCompare(b.date) || Number(b.allDay) - Number(a.allDay) || (a.startTime ?? '').localeCompare(b.startTime ?? '') || a.title.localeCompare(b.title));
}

async function saveRecord<T extends { id: string; updatedAt: string }>(collection: keyof PreviewDatabase, input: Omit<T, 'id' | 'updatedAt'> & { id?: string }): Promise<T> {
  const db = readPreview();
  const record = { ...input, id: input.id ?? crypto.randomUUID(), updatedAt: new Date().toISOString() } as T;
  const records = db[collection] as unknown as T[];
  const index = records.findIndex(item => item.id === record.id);
  if (input.id && index < 0) throw new Error('다른 창에서 삭제된 항목이에요. 창을 닫고 다시 확인해 주세요.');
  if (index < 0) records.push(record); else records[index] = record;
  writePreview(db);
  return record;
}

async function deleteRecord(collection: keyof PreviewDatabase, id: string): Promise<void> {
  const db = readPreview();
  const records = db[collection];
  const index = records.findIndex(item => item.id === id);
  if (index < 0) throw new Error('이미 삭제된 항목이에요. 창을 닫고 다시 확인해 주세요.');
  if (index >= 0) records.splice(index, 1);
  writePreview(db);
}

export const api = {
  async listEvents(from: string, to: string): Promise<CalendarEvent[]> {
    return sortEvents(isNative ? await invoke<CalendarEvent[]>('list_events', { from, to }) : readPreview().events.filter(event => event.date >= from && event.date <= to));
  },
  async saveEvent(input: EventInput): Promise<CalendarEvent> {
    const error = validateEvent(input);
    if (error) throw new Error(error);
    const clean = { ...input, title: input.title.trim(), startTime: input.allDay ? null : input.startTime, endTime: input.allDay ? null : input.endTime };
    return isNative ? invoke('save_event', { input: clean }) : saveRecord<CalendarEvent>('events', clean);
  },
  async deleteEvent(id: string): Promise<void> { return isNative ? invoke('delete_event', { id }) : deleteRecord('events', id); },
  async listTasks(): Promise<Task[]> {
    return isNative ? invoke('list_tasks') : readPreview().tasks.sort((a, b) => Number(a.completed) - Number(b.completed) || Number(a.date === null) - Number(b.date === null) || (a.date ?? '').localeCompare(b.date ?? '') || a.title.localeCompare(b.title));
  },
  async saveTask(input: TaskInput): Promise<Task> {
    if (!input.title.trim()) throw new Error('할 일의 제목을 입력해 주세요.');
    if ([...input.title.trim()].length > 200) throw new Error('할 일의 제목은 200자까지 입력할 수 있어요.');
    if (input.date && !validateDate(input.date)) throw new Error('올바른 날짜를 선택해 주세요.');
    const clean = { ...input, title: input.title.trim() };
    return isNative ? invoke('save_task', { input: clean }) : saveRecord<Task>('tasks', clean);
  },
  async deleteTask(id: string): Promise<void> { return isNative ? invoke('delete_task', { id }) : deleteRecord('tasks', id); },
  async listNotes(): Promise<Note[]> { return isNative ? invoke('list_notes') : readPreview().notes; },
  async saveNote(input: NoteInput): Promise<Note> {
    if (!input.title.trim()) throw new Error('메모 제목을 입력해 주세요.');
    if ([...input.title.trim()].length > 200) throw new Error('메모 제목은 200자까지 입력할 수 있어요.');
    if ([...input.body].length > 20000) throw new Error('메모는 20,000자까지 입력할 수 있어요.');
    const clean = { ...input, title: input.title.trim() };
    return isNative ? invoke('save_note', { input: clean }) : saveRecord<Note>('notes', clean);
  },
  async deleteNote(id: string): Promise<void> { return isNative ? invoke('delete_note', { id }) : deleteRecord('notes', id); },
  async getAppInfo(): Promise<AppInfo> {
    return isNative ? invoke('get_app_info') : { storagePath: '이 브라우저의 localStorage', version: '미리보기', desktopAttached: false, desktopMessage: '브라우저 미리보기에서는 바탕화면 위젯을 사용할 수 없어요.', widgetLocked: true, widgetVisible: false, widgetOpacity: previewWidgetOpacity };
  },
  async showManager(date?: string): Promise<void> {
    if (isNative) return invoke('show_manager', { date });
    const url = new URL(window.location.href);
    url.searchParams.delete('view');
    if (date) url.searchParams.set('date', date);
    window.open(url, 'timeschedule-manager');
  },
  async showWidget(): Promise<void> {
    if (isNative) return invoke('show_widget');
    const url = new URL(window.location.href);
    url.searchParams.set('view', 'widget');
    window.open(url, 'timeschedule-widget', 'width=900,height=560');
  },
  async hideWidget(): Promise<void> { if (isNative) return invoke('hide_widget'); },
  async setWidgetLocked(locked: boolean): Promise<void> { if (isNative) return invoke('set_widget_locked', { locked }); },
  async setWidgetOpacity(opacity: number): Promise<void> {
    if (!Number.isInteger(opacity) || opacity < 40 || opacity > 100) throw new Error('불투명도는 40~100%로 설정해 주세요.');
    if (isNative) return invoke('set_widget_opacity', { opacity });
    previewWidgetOpacity = opacity;
  },
};

export async function onDataChange(callback: () => void): Promise<() => void> {
  const local = () => callback();
  const storage = (event: StorageEvent) => { if (event.key === storageKey) callback(); };
  window.addEventListener(changeEvent, local);
  window.addEventListener('storage', storage);
  let unlisten: (() => void) | undefined;
  try { if (isNative) unlisten = await listen('data-changed', callback); }
  catch (error) { window.removeEventListener(changeEvent, local); window.removeEventListener('storage', storage); throw error; }
  return () => { window.removeEventListener(changeEvent, local); window.removeEventListener('storage', storage); unlisten?.(); };
}

export async function onSelectDate(callback: (date: string) => void): Promise<() => void> {
  return isNative ? listen<string>('select-date', event => callback(event.payload)) : () => {};
}

export async function onDesktopChange(callback: () => void): Promise<() => void> {
  return isNative ? listen('desktop-changed', callback) : () => {};
}

export async function onAppError(callback: (message: string) => void): Promise<() => void> {
  return isNative ? listen<string>('app-error', event => callback(event.payload)) : () => {};
}
