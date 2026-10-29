export function dateKey(date = new Date()): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

export function parseDate(key: string): Date {
  const [year, month, day] = key.split('-').map(Number);
  const date = new Date(2000, month - 1, day, 12);
  date.setFullYear(year);
  return date;
}

export function addDays(key: string, delta: number): string {
  const date = parseDate(key);
  date.setDate(date.getDate() + delta);
  return dateKey(date);
}

export function weekAfterSunday(sunday: string): { from: string; to: string } {
  if (parseDate(sunday).getDay() !== 0) throw new Error('주간 선택 기준은 일요일이어야 합니다.');
  return { from: addDays(sunday, 1), to: addDays(sunday, 7) };
}

export function weekContaining(key: string): { from: string; to: string } {
  const offset = (parseDate(key).getDay() + 6) % 7;
  return { from: addDays(key, -offset), to: addDays(key, 6 - offset) };
}

export function monthStart(key: string): Date {
  const date = parseDate(key);
  return new Date(date.getFullYear(), date.getMonth(), 1, 12);
}

export function shiftMonth(date: Date, delta: number): Date {
  return new Date(date.getFullYear(), date.getMonth() + delta, 1, 12);
}

export function calendarDays(month: Date): { key: string; day: number; currentMonth: boolean; weekday: number }[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1, 12);
  first.setDate(first.getDate() - first.getDay());
  return Array.from({ length: 42 }, (_, index) => {
    const date = new Date(first);
    date.setDate(date.getDate() + index);
    return { key: dateKey(date), day: date.getDate(), currentMonth: date.getMonth() === month.getMonth(), weekday: date.getDay() };
  });
}

export const weekdayNames = ['일', '월', '화', '수', '목', '금', '토'];

export function readableDate(key: string): string {
  return parseDate(key).toLocaleDateString('ko-KR', { month: 'long', day: 'numeric', weekday: 'long' });
}
