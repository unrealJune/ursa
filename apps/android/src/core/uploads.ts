import { File, Paths } from 'expo-file-system';
import { AssetField, MediaType, Query, Asset as MediaAsset, requestPermissionsAsync } from 'expo-media-library';
import { errorText, type UploadMeta, Ursa, withProgress } from 'ursa-core';

import { upsert } from './library';
import { session } from './session';
import { createStore, singleFlight, useStore } from './state';
import { log } from './telemetry';

export type QueueItem = { uri: string; meta: UploadMeta; assetId?: string };

type UploadsState = {
  /** Files in this batch, finished ones included. */
  total: number;
  done: number;
  failed: number;
  current: { name: string; done: number; total: number } | null;
  lastError: string | null;
  /** Looking through the camera roll for photos not backed up yet. */
  scanning: boolean;
};

const IDLE: UploadsState = { total: 0, done: 0, failed: 0, current: null, lastError: null, scanning: false };

export const uploads = createStore<UploadsState>(IDLE);

export function useUploads<U>(select: (s: UploadsState) => U): U {
  return useStore(uploads, select);
}

/** Which camera-roll assets are backed up (asset id → file id), on this phone only. */
const backedUp = {
  file: () => new File(Paths.document, 'backed-up.json'),
  map: null as Record<string, string> | null,
  async load(): Promise<Record<string, string>> {
    if (this.map) return this.map;
    try {
      const f = this.file();
      this.map = f.exists ? (JSON.parse(await f.text()) as Record<string, string>) : {};
    } catch {
      this.map = {};
    }
    return this.map;
  },
  save() {
    const f = this.file();
    if (!f.exists) f.create();
    f.write(JSON.stringify(this.map ?? {}));
  },
};

let queue: QueueItem[] = [];
let draining = false;
/** Bumped when the account goes away, so an upload already running does not record itself. */
let generation = 0;

/**
 * Uploads are staged: the core stores each file but writes the library index once per batch,
 * since writing it per file re-uploaded the whole index for every photo of a backup. A staged
 * file shows in the library at once, but is only kept for good when its batch is committed, so
 * a camera-roll photo counts as backed up only then; one the app dies before committing is
 * found again by the next backup and uploaded again.
 */
const BATCH_FILES = 25;
const BATCH_MS = 15_000;
/** Staged since the last commit, with the camera-roll asset each came from. */
let uncommitted: { assetId?: string; fileId: string }[] = [];
let commitTimer: ReturnType<typeof setTimeout> | null = null;

/** Writes the staged uploads into the library, and records what is now backed up. */
export const commitUploads = singleFlight(async () => {
  if (commitTimer) clearTimeout(commitTimer);
  commitTimer = null;
  if (uncommitted.length === 0) return;
  const gen = generation;
  const batch = uncommitted;
  try {
    // 0 when another library change (a delete) already wrote them: they are committed either way.
    await Ursa.commitUploads();
    if (gen !== generation) return;
    uncommitted = uncommitted.filter((u) => !batch.includes(u));
    const map = await backedUp.load();
    for (const u of batch) if (u.assetId) map[u.assetId] = u.fileId;
    backedUp.save();
  } catch (e) {
    if (gen !== generation) return;
    uploads.set({ lastError: `Saving the library: ${errorText(e)}` });
    log('WARN', `upload commit failed: ${errorText(e)}`, { files: batch.length });
    // Kept for the next try: the next upload, the queue emptying, or the app leaving the screen.
  }
});

function staged(assetId: string | undefined, fileId: string) {
  uncommitted.push({ assetId, fileId });
  if (uncommitted.length >= BATCH_FILES) commitUploads();
  else if (!commitTimer) commitTimer = setTimeout(() => commitUploads(), BATCH_MS);
}

async function drain() {
  if (draining) return;
  draining = true;
  const gen = generation;
  try {
    for (let job = queue.shift(); job && gen === generation; job = queue.shift()) {
      const name = job.meta.name ?? job.uri.split('/').pop() ?? 'file';
      uploads.set({ current: { name, done: 0, total: 1 } });
      const started = Date.now();
      try {
        const item = await withProgress(
          (task) => Ursa.uploadStaged(job.uri, job.meta, task),
          (done, total) => gen === generation && uploads.set({ current: { name, done, total } }),
        );
        if (gen !== generation) break;
        // The core logs where its own part of the time went ("upload done"); this adds the phone's
        // part before it. No names: the type, the size and the times.
        log('INFO', 'upload: finished', {
          mime: item.mime,
          bytes: item.size,
          ms: Date.now() - started,
          prepareMs: Math.round(item.prepareMs ?? 0),
          backup: !!job.assetId,
        });
        const { prepareMs: _prepareMs, ...kept } = item;
        upsert(kept);
        staged(job.assetId, item.fileId);
        uploads.set((u) => ({ done: u.done + 1 }));
      } catch (e) {
        if (gen !== generation) break;
        uploads.set((u) => ({ failed: u.failed + 1, lastError: `${name}: ${errorText(e)}` }));
        log('WARN', `upload failed: ${errorText(e)}`, { mime: job.meta.mime });
      }
    }
  } finally {
    draining = false;
    if (gen === generation) {
      uploads.set({ current: null });
      // The queue is empty: write what is staged now rather than when the timer fires.
      await commitUploads();
    }
    // Files queued under a new account while the old account's last upload was finishing.
    else if (queue.length) drain();
  }
}

export function enqueue(files: QueueItem[]) {
  if (files.length === 0) return;
  const idle = !draining && queue.length === 0;
  queue.push(...files);
  uploads.set((u) => (idle ? { ...IDLE, scanning: u.scanning, total: files.length } : { total: u.total + files.length }));
  drain();
}

/** Queues every camera-roll photo (and video) not yet backed up, newest first. */
export const backupNow = singleFlight(async () => {
  uploads.set({ scanning: true });
  try {
    const { backupVideos } = session.get().prefs;
    const kinds: ('photo' | 'video')[] = backupVideos ? ['photo', 'video'] : ['photo'];
    const perm = await requestPermissionsAsync(false, kinds);
    if (!perm.granted) throw new Error('Ursa needs access to your photos to back them up.');
    const map = await backedUp.load();
    const queued = new Set(queue.map((q) => q.assetId).filter(Boolean));
    const types = backupVideos ? [MediaType.IMAGE, MediaType.VIDEO] : [MediaType.IMAGE];
    const fresh: QueueItem[] = [];
    const PAGE = 500;
    for (let offset = 0; ; offset += PAGE) {
      const page = await new Query()
        .within(AssetField.MEDIA_TYPE, types)
        .orderBy({ key: AssetField.CREATION_TIME, ascending: false })
        .limit(PAGE)
        .offset(offset)
        .exeForMetadata();
      for (const a of page) {
        if (map[a.id] || queued.has(a.id)) continue;
        const uri = await new MediaAsset(a.id).getUri();
        fresh.push({
          uri,
          assetId: a.id,
          meta: {
            name: a.filename ?? undefined,
            createdMs: a.creationTime ?? undefined,
            modifiedMs: a.modificationTime ?? undefined,
            width: a.width ?? undefined,
            height: a.height ?? undefined,
          },
        });
      }
      if (page.length < PAGE) break;
    }
    enqueue(fresh);
  } catch (e) {
    uploads.set({ lastError: errorText(e) });
    log('WARN', `backup scan failed: ${errorText(e)}`);
  } finally {
    uploads.set({ scanning: false });
  }
});

/** The account went away: drop the queue and forget what was backed up under it. */
export function resetUploads() {
  generation += 1;
  queue = [];
  uncommitted = [];
  if (commitTimer) clearTimeout(commitTimer);
  commitTimer = null;
  backedUp.map = {};
  backedUp.save();
  uploads.set(IDLE);
}
