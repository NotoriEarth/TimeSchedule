export function dateKey(date = new Date()): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

export function parseDate(key: string): Date {
  const [year, month, day] = key.split('-').map(Number);
  return new Date(year, month - 1, day, 12);
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
