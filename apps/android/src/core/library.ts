import { errorText, type LibraryItem, Ursa } from 'ursa-core';

import { saveFile } from './files';
import { syncAccount } from './session';
import { createStore, singleFlight, useStore } from './state';
import { log } from './telemetry';

/** A batch the person started on several files at once (delete, save). */
export type Bulk = { label: string; done: number; failed: number; total: number };

type LibraryState = {
  /** Every file, newest first. */
  items: LibraryItem[];
  /** The list has been read at least once since the account opened. */
  loaded: boolean;
  loading: boolean;
  /** The last sync with the nodes failed; the list is what this phone already knew. */
  error: string | null;
  bulk: Bulk | null;
  /** What the last batch could not do. */
  bulkError: string | null;
};

const EMPTY: LibraryState = { items: [], loaded: false, loading: false, error: null, bulk: null, bulkError: null };

export const library = createStore<LibraryState>(EMPTY);

export function useLibrary<U>(select: (s: LibraryState) => U): U {
  return useStore(library, select);
}

const newestFirst = (a: LibraryItem, b: LibraryItem) => b.createdMs - a.createdMs || (a.fileId < b.fileId ? -1 : 1);

let lastSync = 0;

/**
 * Reads the library. With `fromNodes` it first re-reads the account from its nodes (what other
 * devices changed); when they do not answer, the list is still what this phone knows, with a
 * notice, and the app stays where it is.
 */
export const loadLibrary = singleFlight(async ({ fromNodes }: { fromNodes: boolean }) => {
  library.set({ loading: true });
  let error: string | null = null;
  if (fromNodes) {
    try {
      await syncAccount();
      lastSync = Date.now();
    } catch (e) {
      error = errorText(e);
      log('WARN', `library: ${error}`);
    }
  }
  try {
    const items = await Ursa.list();
    items.sort(newestFirst);
    library.set({ items, loaded: true, error });
  } catch (e) {
    library.set({ error: errorText(e) });
    log('WARN', `library: ${errorText(e)}`);
  } finally {
    library.set({ loading: false });
  }
});

/** Syncs with the nodes when the last sync is older than `maxAgeMs`. */
export function syncIfStale(maxAgeMs: number) {
  if (Date.now() - lastSync > maxAgeMs) loadLibrary({ fromNodes: true });
}

/** The account just opened (which read it from the nodes). */
export function libraryOpened() {
  lastSync = Date.now();
  return loadLibrary({ fromNodes: false });
}

export function resetLibrary() {
  lastSync = 0;
  library.set(EMPTY);
}

/** Adds (or replaces) a file this phone has just uploaded. */
export function upsert(item: LibraryItem) {
  library.set((s) => ({ items: [item, ...s.items.filter((i) => i.fileId !== item.fileId)].sort(newestFirst) }));
}

/** Runs `run` on each file, a few at a time, with the batch's progress in `bulk`. */
async function batch(label: string, items: LibraryItem[], parallel: number, run: (item: LibraryItem) => Promise<void>): Promise<LibraryItem[]> {
  if (library.get().bulk) throw new Error('Another batch is still running.');
  library.set({ bulk: { label, done: 0, failed: 0, total: items.length }, bulkError: null });
  const done: LibraryItem[] = [];
  const errors: string[] = [];
  let next = 0;
  const worker = async () => {
    for (let i = next++; i < items.length; i = next++) {
      const item = items[i];
      try {
        await run(item);
        done.push(item);
        library.set((s) => ({ bulk: s.bulk && { ...s.bulk, done: s.bulk.done + 1 } }));
      } catch (e) {
        errors.push(`${item.name}: ${errorText(e)}`);
        library.set((s) => ({ bulk: s.bulk && { ...s.bulk, failed: s.bulk.failed + 1 } }));
      }
    }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(parallel, items.length) }, worker));
  } finally {
    library.set({
      bulk: null,
      bulkError: errors.length ? `${errors.length} of ${items.length} failed. ${errors[0]}${errors.length > 1 ? ' …' : ''}` : null,
    });
  }
  return done;
}

/** Deletes files from every device and node. Resolves with the ids that were deleted. */
export async function removeMany(items: LibraryItem[]): Promise<string[]> {
  const gone = new Set<string>();
  await batch(items.length === 1 ? 'Deleting' : `Deleting ${items.length} items`, items, 3, async (item) => {
    await Ursa.delete(item.fileId);
    gone.add(item.fileId);
    library.set((s) => ({ items: s.items.filter((i) => i.fileId !== item.fileId) }));
  });
  return [...gone];
}

export async function remove(item: LibraryItem) {
  await Ursa.delete(item.fileId);
  library.set((s) => ({ items: s.items.filter((i) => i.fileId !== item.fileId) }));
}

/** Downloads files into the phone's gallery (photos, videos) or Downloads (everything else). */
export async function saveMany(items: LibraryItem[]): Promise<number> {
  const done = await batch(items.length === 1 ? 'Saving to this phone' : `Saving ${items.length} items to this phone`, items, 2, (item) => saveFile(item));
  return done.length;
}

export function clearBulkError() {
  library.set({ bulkError: null });
}
