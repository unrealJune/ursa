export function bytes(n: number | null | undefined): string {
  if (n == null) return '';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = n;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i += 1;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
const dayYearFmt = new Intl.DateTimeFormat(undefined, { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' });
const monthFmt = new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric' });
const fullFmt = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });

/** "Today", "Yesterday", "Mon, 3 Nov", or with the year when it is not this year. */
export function dayLabel(ms: number, now = Date.now()): string {
  const d = new Date(ms);
  const today = new Date(now);
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((startOf(today) - startOf(d)) / 86_400_000);
  if (diff === 0) return 'Today';
  if (diff === 1) return 'Yesterday';
  return d.getFullYear() === today.getFullYear() ? dayFmt.format(d) : dayYearFmt.format(d);
}

export function monthLabel(ms: number): string {
  return monthFmt.format(new Date(ms));
}

export function dateTime(ms: number): string {
  return fullFmt.format(new Date(ms));
}

export function dayKey(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

export function shortId(id: string): string {
  return id.slice(0, 10);
}

export function isMedia(mime: string): boolean {
  return mime.startsWith('image/') || mime.startsWith('video/');
}

/** Material icon for a file type. */
export function fileIcon(mime: string, name: string): string {
  if (mime.startsWith('image/')) return 'image';
  if (mime.startsWith('video/')) return 'movie';
  if (mime.startsWith('audio/')) return 'audiotrack';
  if (mime === 'application/pdf') return 'picture-as-pdf';
  if (mime.startsWith('text/')) return 'description';
  if (/\.(zip|7z|rar|tar|gz)$/i.test(name)) return 'folder-zip';
  if (/\.(xlsx?|csv|ods)$/i.test(name)) return 'table-chart';
  if (/\.(docx?|odt|rtf)$/i.test(name)) return 'article';
  if (/\.(pptx?|odp|key)$/i.test(name)) return 'slideshow';
  return 'insert-drive-file';
}
