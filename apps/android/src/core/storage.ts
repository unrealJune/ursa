import { errorText, Ursa, withProgress } from 'ursa-core';

import { createStore, useStore, watch } from './state';

/**
 * Protection and space: the core's storage report (the same JSON the Windows app reads), and
 * switching the library to another protection. A switch runs here, not in a screen, so it keeps
 * going while the rest of the app is used, and one cut short (the app closed) picks up again at
 * the next start.
 */

/** One protection choice: any `k` of `n` pieces rebuild a file. */
export type Choice = {
  k: number;
  n: number;
  /** Nodes that can be down with every file still readable. */
  survives: number;
  limitBytes: number;
  /** Expected hours a year a file is out of reach, at the pool's measured uptime. */
  offlineHours: number | null;
  /** The pool has enough people (failure domains) for it. */
  available: boolean;
  recommended: boolean;
  /** Extra pool space switching to it takes while it runs, and whether the pool has it. */
  switchExtraBytes: number;
  switchFits: boolean;
};

export type Member = { name: string; own: boolean; reachable: boolean; up30d: number | null; usedBytes: number; givesYou: number | null };
export type Hosted = { name: string; usedBytes: number; quotaBytes: number };
export type Disk = { totalBytes: number; freeBytes: number; yoursBytes: number; friendsBytes: number; reservedBytes: number };
export type Month = { month: string; bytes: number };

export type StorageReport = {
  k: number;
  n: number;
  chosen: boolean;
  files: number;
  pendingFiles: number;
  leftoverPieces: number;
  storedBytes: number;
  kinds: { photos: number; videos: number; files: number };
  limitBytes: number;
  choices: Choice[];
  giveBytes: number;
  homeBytes: number;
  friendsBytes: number;
  members: Member[];
  hosting: Hosted[];
  disk: Disk | null;
  growth: Month[];
};

function parse(json: string): StorageReport {
  const r: any = JSON.parse(json);
  return {
    k: r.protection.k,
    n: r.protection.n,
    chosen: r.chosen,
    files: r.files,
    pendingFiles: r.pending_files,
    leftoverPieces: r.leftover_pieces ?? 0,
    storedBytes: r.stored_bytes,
    kinds: { photos: r.kinds.photos, videos: r.kinds.videos, files: r.kinds.files },
    limitBytes: r.limit_bytes,
    choices: r.choices.map(
      (c: any): Choice => ({
        k: c.k,
        n: c.n,
        survives: c.survives,
        limitBytes: c.limit_bytes,
        offlineHours: c.offline_hours ?? null,
        available: c.available,
        recommended: c.recommended,
        switchExtraBytes: c.switch_extra_bytes ?? 0,
        switchFits: !!c.switch_fits,
      }),
    ),
    giveBytes: r.give_bytes,
    homeBytes: r.home_bytes,
    friendsBytes: r.friends_bytes,
    members: r.members.map(
      (m: any): Member => ({
        name: m.name,
        own: m.own,
        reachable: m.reachable,
        up30d: m.up_30d ?? null,
        usedBytes: m.used_bytes,
        givesYou: m.gives_you ?? null,
      }),
    ),
    hosting: r.hosting.map((h: any): Hosted => ({ name: h.name, usedBytes: h.used_bytes, quotaBytes: h.quota_bytes })),
    disk: r.disk
      ? {
          totalBytes: r.disk.total_bytes,
          freeBytes: r.disk.free_bytes,
          yoursBytes: r.disk.yours_bytes,
          friendsBytes: r.disk.friends_bytes,
          reservedBytes: r.disk.reserved_bytes,
        }
      : null,
    growth: r.growth.map((m: any): Month => ({ month: m.month, bytes: m.bytes })),
  };
}

/** The storage report; `extra` adds a shape being considered (a custom k of n) to the choices. */
export async function loadStorage(extra?: { k: number; n: number } | null): Promise<StorageReport> {
  return parse(await Ursa.storage(extra?.k ?? null, extra?.n ?? null));
}

// ---------------------------------------------------------------------------------------------
// Names

export function choiceName(c: { k: number; n: number }): string {
  if (c.k === 1) return `${c.n} copies`;
  return `${Number((c.n / c.k).toFixed(2))}×`;
}

/** `3 of 5`, or nothing for whole copies. */
export function choiceShape(c: { k: number; n: number }): string {
  return c.k === 1 ? '' : `${c.k} of ${c.n}`;
}

export function offlineText(c: Choice): string {
  if (!c.available || c.offlineHours == null) return '—';
  const h = c.offlineHours;
  if (h < 1 / 60) return '< 1 min/yr';
  if (h < 1) return `${Math.round(h * 60)} min/yr`;
  return `${h < 10 ? h.toFixed(1) : Math.round(h)} h/yr`;
}

export const same = (a: { k: number; n: number }, b: { k: number; n: number }) => a.k === b.k && a.n === b.n;

// ---------------------------------------------------------------------------------------------
// Switching

type SwitchState = {
  switching: boolean;
  /** 0..1, or null before the first progress report. */
  progress: number | null;
  error: string | null;
  /** Bumped when a switch ends, so screens reload. */
  ended: number;
};

const switchStore = createStore<SwitchState>({ switching: false, progress: null, error: null, ended: 0 });

export function useSwitch<U>(select: (s: SwitchState) => U): U {
  return useStore(switchStore, select);
}

/** Calls `fn` each time a switch ends; returns the unsubscribe. */
export function onSwitchEnded(fn: () => void): () => void {
  return watch(switchStore, (s) => s.ended, () => fn());
}

/** `quiet`: a resume at start, whose failures wait for the next start instead of showing. */
async function recodeInBackground(quiet = false) {
  if (switchStore.get().switching) return;
  switchStore.set({ switching: true, progress: null, error: null });
  try {
    const r = await withProgress(
      (task) => Ursa.recode(task),
      (done, total) => switchStore.set({ progress: total > 0 ? done / total : null }),
    );
    if (r.failures.length && !quiet) switchStore.set({ error: `${r.failures.length} file(s) could not be moved. Switch again when every node is up.` });
  } catch (e) {
    if (!quiet) switchStore.set({ error: errorText(e) });
  } finally {
    switchStore.set({ switching: false, ended: switchStore.get().ended + 1 });
  }
}

/** Chooses `k` of `n` and moves the library to it in the background. */
export async function startSwitch(k: number, n: number) {
  if (switchStore.get().switching) return;
  await Ursa.setProtection(k, n);
  void recodeInBackground();
}

let resumed = false;

/**
 * Once the account is open: finishes a switch that was cut short (and clears what it left), then
 * sweeps pieces nothing uses that are old enough to be sure of. Quiet; the next start tries again.
 */
export async function resumeStorage() {
  if (resumed) return;
  resumed = true;
  try {
    const r = await loadStorage();
    if (r.chosen && (r.pendingFiles > 0 || r.leftoverPieces > 0)) await recodeInBackground(true);
    await Ursa.collectGarbage(null);
  } catch {
    resumed = false;
  }
}
