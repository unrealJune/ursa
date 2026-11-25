import type { LibraryItem } from 'ursa-core';

import { dayKey, dayLabel, monthLabel } from '@/format';

export const GAP = 2;
export const HEADER_HEIGHT = 48;
export const BOTTOM_PADDING = 24;

/** Zoom levels, tightest last: the size a thumbnail aims for, in dp. */
const CELL = [200, 132, 96, 64, 40];
export const MAX_ZOOM = CELL.length - 1;
/** From this level on, the grid groups by month instead of by day. */
const MONTHS_FROM = 3;

export function clampZoom(z: number): number {
  return Math.max(0, Math.min(MAX_ZOOM, Math.round(z)));
}

export type Header = { type: 'header'; key: string; label: string; top: number; height: number; start: number; end: number };
export type Row = { type: 'row'; key: string; top: number; height: number; start: number; items: LibraryItem[] };
export type Entry = Header | Row;

/**
 * The grid, laid out in full: every header and row with its offset, so the scrubber, the zoom
 * anchor and drag-select can map between scroll offsets, dates and items without measuring.
 */
export type Layout = {
  entries: Entry[];
  columns: number;
  size: number;
  /** The content's height. */
  height: number;
};

export function layout(items: LibraryItem[], width: number, zoom: number): Layout {
  const columns = Math.max(2, Math.round(width / CELL[clampZoom(zoom)]));
  const size = (width - GAP * (columns - 1)) / columns;
  const months = zoom >= MONTHS_FROM;
  const entries: Entry[] = [];
  let top = 0;
  let group = '';
  let header: Header | null = null;
  for (let i = 0; i < items.length; ) {
    const item = items[i];
    const d = new Date(item.createdMs);
    const k = months ? `${d.getFullYear()}-${d.getMonth()}` : dayKey(item.createdMs);
    if (k !== group) {
      group = k;
      if (header) header.end = i;
      header = { type: 'header', key: `h-${k}`, label: months ? monthLabel(item.createdMs) : dayLabel(item.createdMs), top, height: HEADER_HEIGHT, start: i, end: i };
      entries.push(header);
      top += HEADER_HEIGHT;
    }
    const row: LibraryItem[] = [];
    const start = i;
    while (i < items.length && row.length < columns) {
      const next = new Date(items[i].createdMs);
      const nk = months ? `${next.getFullYear()}-${next.getMonth()}` : dayKey(items[i].createdMs);
      if (nk !== group) break;
      row.push(items[i]);
      i += 1;
    }
    entries.push({ type: 'row', key: `r-${row[0].fileId}`, top, height: size + GAP, start, items: row });
    top += size + GAP;
  }
  if (header) header.end = items.length;
  return { entries, columns, size, height: top + BOTTOM_PADDING };
}

/** The entry at content offset `y` (the last one starting at or above it). */
export function entryAt(l: Layout, y: number): number {
  let lo = 0;
  let hi = l.entries.length - 1;
  if (hi < 0) return -1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (l.entries[mid].top <= y) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/**
 * The item under a point (`y` in content coordinates), for drag-select: past the end of a short
 * row counts as its last item, and a header as the first item after it.
 */
export function itemAt(l: Layout, x: number, y: number): number | null {
  const e = l.entries[entryAt(l, y)];
  if (!e) return null;
  if (e.type === 'header') return e.start;
  const col = Math.max(0, Math.min(e.items.length - 1, Math.floor(x / (l.size + GAP))));
  return e.start + col;
}

/** The offset of the row holding item `index`. */
export function rowTop(l: Layout, index: number): number {
  for (const e of l.entries) {
    if (e.type === 'row' && index >= e.start && index < e.start + e.items.length) return e.top;
  }
  return 0;
}

/** When the photos at content offset `y` were taken. */
export function timeAt(l: Layout, y: number): number | null {
  for (let i = entryAt(l, y); i >= 0 && i < l.entries.length; i++) {
    const e = l.entries[i];
    if (e.type === 'row') return e.items[0].createdMs;
  }
  return null;
}

/** Where each year starts, for the scrubber's marks. */
export function yearMarks(l: Layout): { year: number; top: number }[] {
  const marks: { year: number; top: number }[] = [];
  let last = NaN;
  for (const e of l.entries) {
    if (e.type !== 'row') continue;
    const y = new Date(e.items[0].createdMs).getFullYear();
    if (y !== last) {
      marks.push({ year: y, top: e.top });
      last = y;
    }
  }
  return marks;
}
