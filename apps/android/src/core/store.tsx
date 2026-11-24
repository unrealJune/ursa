import { File, Paths } from 'expo-file-system';
import { AssetField, MediaType, Query, Asset as MediaAsset, requestPermissionsAsync } from 'expo-media-library';
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { type AccountKeys, errorText, type LibraryItem, type Status, Ursa, type UploadMeta, withProgress } from 'ursa-core';

import { accountKey, loadPrefs, type Prefs, relayToken, savePrefs } from './settings';
import { forgetThumbs } from './thumbs';

/**
 * Where the app is:
 * - `starting`: the core is coming up
 * - `failed`: it could not start (the network endpoint, usually a bad relay URL)
 * - `offline`: this phone holds an account but none of its nodes answered
 * - `welcome` / `needs-node`: onboarding; an account needs at least one node of its own
 * - `ready`: the library
 */
export type Phase = 'starting' | 'failed' | 'offline' | 'welcome' | 'needs-node' | 'ready';

export type QueueItem = { uri: string; meta: UploadMeta; assetId?: string };

export type Uploads = {
  /** Files in this batch, finished ones included. */
  total: number;
  done: number;
  failed: number;
  current: { name: string; done: number; total: number } | null;
  lastError: string | null;
};

const IDLE: Uploads = { total: 0, done: 0, failed: 0, current: null, lastError: null };

type Ctx = {
  phase: Phase;
  status: Status | null;
  problem: string | null;
  prefs: Prefs;
  items: LibraryItem[];
  libLoading: boolean;
  libError: string | null;
  uploads: Uploads;
  backupScanning: boolean;
  retry(): Promise<void>;
  refreshStatus(): Promise<void>;
  refreshLibrary(): Promise<void>;
  adopt(keys: AccountKeys): Promise<void>;
  signOut(): Promise<void>;
  setNetwork(relays: string[], token: string | null): Promise<void>;
  setPrefs(p: Partial<Prefs>): Promise<void>;
  enqueue(files: QueueItem[]): void;
  remove(fileId: string): Promise<void>;
  backupNow(): Promise<void>;
};

const UrsaContext = createContext<Ctx | null>(null);

export function useUrsa(): Ctx {
  const c = useContext(UrsaContext);
  if (!c) throw new Error('useUrsa outside UrsaProvider');
  return c;
}

/** Which camera-roll assets are backed up (asset id → file id), on this phone only. */
const backedUp = {
  file: () => new File(Paths.document, 'backed-up.json'),
  async load(): Promise<Record<string, string>> {
    try {
      const f = this.file();
      return f.exists ? (JSON.parse(await f.text()) as Record<string, string>) : {};
    } catch {
      return {};
    }
  },
  save(map: Record<string, string>) {
    const f = this.file();
    if (!f.exists) f.create();
    f.write(JSON.stringify(map));
  },
};

function phaseOf(status: Status | null, hadKey: boolean): Phase {
  if (!status) return 'starting';
  if (!status.hasAccount) return hadKey && status.openError ? 'offline' : 'welcome';
  return status.homeNodes.length === 0 ? 'needs-node' : 'ready';
}

export function UrsaProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [hadKey, setHadKey] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [prefs, setPrefsState] = useState<Prefs>({ relays: [], backup: false, backupVideos: true });
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [libLoading, setLibLoading] = useState(false);
  const [libError, setLibError] = useState<string | null>(null);
  const [uploads, setUploads] = useState<Uploads>(IDLE);
  const [backupScanning, setBackupScanning] = useState(false);

  const queue = useRef<QueueItem[]>([]);
  const draining = useRef(false);
  const backupMap = useRef<Record<string, string> | null>(null);

  const start = useCallback(async () => {
    setFailed(false);
    setProblem(null);
    setStatus(null);
    try {
      const [p, key, token] = await Promise.all([loadPrefs(), accountKey.get(), relayToken.get()]);
      setPrefsState(p);
      setHadKey(!!key);
      const st = await Ursa.init(key, p.relays, token);
      setStatus(st);
      setProblem(st.openError);
    } catch (e) {
      setFailed(true);
      setProblem(errorText(e));
    }
  }, []);

  useEffect(() => {
    start();
  }, [start]);

  const phase: Phase = failed ? 'failed' : phaseOf(status, hadKey);

  const refreshStatus = useCallback(async () => {
    setStatus(await Ursa.status());
  }, []);

  const refreshLibrary = useCallback(async () => {
    setLibLoading(true);
    try {
      const list = await Ursa.list();
      list.sort((a, b) => b.createdMs - a.createdMs);
      setItems(list);
      setLibError(null);
    } catch (e) {
      setLibError(errorText(e));
    } finally {
      setLibLoading(false);
    }
  }, []);

  useEffect(() => {
    if (phase === 'ready') refreshLibrary();
  }, [phase, refreshLibrary]);

  const adopt = useCallback(
    async (keys: AccountKeys) => {
      await accountKey.set(keys.accountKey);
      setHadKey(true);
      await refreshStatus();
    },
    [refreshStatus],
  );

  const signOut = useCallback(async () => {
    await Ursa.signOut();
    await accountKey.clear();
    backedUp.save({});
    backupMap.current = {};
    forgetThumbs();
    setItems([]);
    setHadKey(false);
    await refreshStatus();
  }, [refreshStatus]);

  const setPrefs = useCallback(
    async (p: Partial<Prefs>) => {
      const next = { ...prefs, ...p };
      setPrefsState(next);
      await savePrefs(next);
    },
    [prefs],
  );

  const setNetwork = useCallback(
    async (relays: string[], token: string | null) => {
      const next = { ...prefs, relays };
      await savePrefs(next);
      await relayToken.set(token);
      await Ursa.shutdown();
      await start();
    },
    [prefs, start],
  );

  const drain = useCallback(async () => {
    if (draining.current) return;
    draining.current = true;
    try {
      for (let next = queue.current.shift(); next; next = queue.current.shift()) {
        const job = next;
        const name = job.meta.name ?? job.uri.split('/').pop() ?? 'file';
        setUploads((u) => ({ ...u, current: { name, done: 0, total: 1 } }));
        try {
          const item = await withProgress(
            (task) => Ursa.upload(job.uri, job.meta, task),
            (done, total) => setUploads((u) => ({ ...u, current: { name, done, total } })),
          );
          setItems((list) => [item, ...list.filter((i) => i.fileId !== item.fileId)].sort((a, b) => b.createdMs - a.createdMs));
          if (job.assetId) {
            const map = backupMap.current ?? (backupMap.current = await backedUp.load());
            map[job.assetId] = item.fileId;
            backedUp.save(map);
          }
          setUploads((u) => ({ ...u, done: u.done + 1 }));
        } catch (e) {
          setUploads((u) => ({ ...u, failed: u.failed + 1, lastError: `${name}: ${errorText(e)}` }));
        }
      }
    } finally {
      draining.current = false;
      setUploads((u) => ({ ...u, current: null }));
    }
  }, []);

  const enqueue = useCallback(
    (files: QueueItem[]) => {
      if (files.length === 0) return;
      const idle = !draining.current && queue.current.length === 0;
      queue.current.push(...files);
      setUploads((u) => (idle ? { ...IDLE, total: files.length } : { ...u, total: u.total + files.length }));
      drain();
    },
    [drain],
  );

  const remove = useCallback(async (fileId: string) => {
    await Ursa.delete(fileId);
    setItems((list) => list.filter((i) => i.fileId !== fileId));
  }, []);

  /** Queues every camera-roll photo (and video) not yet backed up, newest first. */
  const backupNow = useCallback(async () => {
    if (backupScanning) return;
    setBackupScanning(true);
    try {
      const kinds: ('photo' | 'video')[] = prefs.backupVideos ? ['photo', 'video'] : ['photo'];
      const perm = await requestPermissionsAsync(false, kinds);
      if (!perm.granted) throw new Error('Ursa needs access to your photos to back them up.');
      const map = backupMap.current ?? (backupMap.current = await backedUp.load());
      const queued = new Set(queue.current.map((q) => q.assetId).filter(Boolean));
      const types = prefs.backupVideos ? [MediaType.IMAGE, MediaType.VIDEO] : [MediaType.IMAGE];
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
      setUploads((u) => ({ ...u, lastError: errorText(e) }));
    } finally {
      setBackupScanning(false);
    }
  }, [backupScanning, prefs.backupVideos, enqueue]);

  // Back up when the library opens and whenever the app comes back to the screen.
  const backupRef = useRef(backupNow);
  backupRef.current = backupNow;
  useEffect(() => {
    if (phase !== 'ready' || !prefs.backup) return;
    backupRef.current();
    const sub = AppState.addEventListener('change', (s) => {
      if (s === 'active') backupRef.current();
    });
    return () => sub.remove();
  }, [phase, prefs.backup]);

  const value = useMemo<Ctx>(
    () => ({
      phase,
      status,
      problem,
      prefs,
      items,
      libLoading,
      libError,
      uploads,
      backupScanning,
      retry: start,
      refreshStatus,
      refreshLibrary,
      adopt,
      signOut,
      setNetwork,
      setPrefs,
      enqueue,
      remove,
      backupNow,
    }),
    [phase, status, problem, prefs, items, libLoading, libError, uploads, backupScanning, start, refreshStatus, refreshLibrary, adopt, signOut, setNetwork, setPrefs, enqueue, remove, backupNow],
  );

  return <UrsaContext.Provider value={value}>{children}</UrsaContext.Provider>;
}
