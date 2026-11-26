import { type Edit, errorText, type FolderEntry, type LibraryItem, Ursa } from 'ursa-core';

import { saveFile } from './files';
import { syncAccount } from './session';
import { createStore, useStore } from './state';
import { log } from './telemetry';

/** A batch the person started on several files at once (delete, save). */
export type Bulk = { label: string; done: number; failed: number; total: number };

type LibraryState = {
  /** Every file, newest first. */
  items: LibraryItem[];
  /** Every folder, as every device shows them. */
  folders: FolderEntry[];
  /** The list has been read at least once since the account opened. */
  loaded: boolean;
  loading: boolean;
  /** A read the person asked for (pull to refresh) is running: the only one that shows a spinner. */
  pulling: boolean;
  /** The last sync with the nodes failed; the list is what this phone already knew. */
  error: string | null;
  bulk: Bulk | null;
  /** What the last batch could not do. */
  bulkError: string | null;
};

const EMPTY: LibraryState = { items: [], folders: [], loaded: false, loading: false, pulling: false, error: null, bulk: null, bulkError: null };

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
 *
 * One read runs at a time, but a sync asked for while a plain read runs is not lost: it runs
 * right after. Before, a pull-to-refresh that landed during a plain read joined it and never
 * asked the nodes, so another device's changes stayed out until the app was restarted.
 */
export function loadLibrary({ fromNodes, reason }: { fromNodes: boolean; reason?: 'pull' | 'timer' | 'resume' }): Promise<void> {
  if (fromNodes) syncWanted = true;
  if (reason === 'pull') {
    log('INFO', 'library: pull to refresh');
    library.set({ pulling: true });
  }
  if (!reading) {
    reading = (async () => {
      try {
        do {
          const sync = syncWanted;
          syncWanted = false;
          await readOnce(sync);
        } while (syncWanted);
      } finally {
        reading = null;
        library.set({ pulling: false });
      }
    })();
  }
  return reading;
}

let reading: Promise<void> | null = null;
let syncWanted = false;

async function readOnce(fromNodes: boolean) {
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
    const { folders, files: items } = await Ursa.tree();
    items.sort(newestFirst);
    library.set({ items, folders, loaded: true, error });
  } catch (e) {
    library.set({ error: errorText(e) });
    log('WARN', `library: ${errorText(e)}`);
  } finally {
    library.set({ loading: false });
  }
}

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

/** How many files one library change deletes. Each change uploads the whole index once. */
const DELETE_CHUNK = 250;

/**
 * Deletes files from every device and node, in as few library changes as it can: each change
 * rewrites the whole index and the account head, so one per file was slow, and running a few at
 * once made them overwrite each other until the core gave up ("the account head kept changing").
 */
export async function removeMany(items: LibraryItem[]) {
  if (library.get().bulk) throw new Error('Another batch is still running.');
  const total = items.length;
  library.set({ bulk: { label: total === 1 ? 'Deleting' : `Deleting ${total} items`, done: 0, failed: 0, total }, bulkError: null });
  let failed = 0;
  let error: string | null = null;
  try {
    for (let i = 0; i < total; i += DELETE_CHUNK) {
      const chunk = items.slice(i, i + DELETE_CHUNK);
      const ids = new Set(chunk.map((c) => c.fileId));
      try {
        await Ursa.deleteMany([...ids]);
        library.set((s) => ({ items: s.items.filter((it) => !ids.has(it.fileId)), bulk: s.bulk && { ...s.bulk, done: s.bulk.done + chunk.length } }));
      } catch (e) {
        failed += chunk.length;
        error = errorText(e);
        log('WARN', `bulk delete failed: ${error}`, { count: chunk.length });
        library.set((s) => ({ bulk: s.bulk && { ...s.bulk, failed: s.bulk.failed + chunk.length } }));
      }
    }
  } finally {
    library.set({ bulk: null, bulkError: failed ? `${failed} of ${total} could not be deleted. ${error}` : null });
  }
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

/** Changes the folder tree in one library change, then reads the library again. */
export async function editTree(edits: Edit[]): Promise<void> {
  if (edits.length === 0) return;
  try {
    await Ursa.edit(JSON.stringify(edits));
  } finally {
    await loadLibrary({ fromNodes: false });
  }
}

/** The folder and every folder under it. */
export function subtree(folders: FolderEntry[], id: string): Set<string> {
  const out = new Set([id]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const f of folders) {
      if (f.parent && out.has(f.parent) && !out.has(f.id)) {
        out.add(f.id);
        grew = true;
      }
    }
  }
  return out;
}
