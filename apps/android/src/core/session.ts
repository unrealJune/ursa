import { type AccountKeys, errorText, type Status, Ursa } from 'ursa-core';

import { accountKey, DEFAULT_PREFS, loadPrefs, type Prefs, relayToken, savePrefs } from './settings';
import { createStore, singleFlight, sleep } from './state';
import { log, startTelemetry } from './telemetry';

/**
 * Where the app is:
 * - `starting`: the core is coming up (and opening the account, which reads it from its nodes)
 * - `failed`: the core could not start (the network endpoint, usually a bad relay URL)
 * - `offline`: this phone holds an account but none of its nodes answered; it keeps retrying
 * - `welcome` / `needs-node`: onboarding; an account needs at least one node of its own
 * - `ready`: the library
 *
 * Only starting the core moves the phase into `offline` or `failed`. Once the library is open, a
 * node that stops answering is the library's problem (a notice over the photos it already has),
 * never a jump to another screen.
 */
export type Phase = 'starting' | 'failed' | 'offline' | 'welcome' | 'needs-node' | 'ready';

type SessionState = {
  phase: Phase;
  status: Status | null;
  /** Why the core did not start, or why the account did not open. */
  problem: string | null;
  prefs: Prefs;
  /** While offline: when the next automatic try is due (ms since epoch), null while one runs. */
  retryAt: number | null;
};

export const session = createStore<SessionState>({ phase: 'starting', status: null, problem: null, prefs: DEFAULT_PREFS, retryAt: null });

export const useSession = session.use;

/** Whether this phone has an account root stored (so a closed account means offline). */
let hasKey = false;
let offlineTries = 0;
let retryTimer: ReturnType<typeof setTimeout> | null = null;

/** Seconds between automatic tries while offline. */
const BACKOFF = [5, 10, 20, 30, 60];

function phaseOf(st: Status): Phase {
  if (!st.hasAccount) return hasKey ? 'offline' : 'welcome';
  return st.homeNodes.length === 0 ? 'needs-node' : 'ready';
}

function apply(st: Status) {
  const phase = phaseOf(st);
  if (phase !== session.get().phase) log('INFO', `phase: ${phase}`, { phase });
  session.set({ status: st, phase, problem: phase === 'offline' ? (st.openError ?? 'None of your nodes answered.') : null });
  clearRetry();
  if (phase === 'offline') {
    const wait = BACKOFF[Math.min(offlineTries, BACKOFF.length - 1)];
    offlineTries += 1;
    session.set({ retryAt: Date.now() + wait * 1000 });
    retryTimer = setTimeout(() => start(), wait * 1000);
  } else {
    offlineTries = 0;
  }
}

function clearRetry() {
  if (retryTimer) clearTimeout(retryTimer);
  retryTimer = null;
  session.set({ retryAt: null });
}

/**
 * Starts the core and opens the stored account. Safe to call again (a retry): the core keeps its
 * endpoint and only tries to open the account again. A phone that has just woken often has no
 * network for a moment, so a failed open is tried twice more, quietly, before the app says the
 * nodes are out of reach.
 */
export const start = singleFlight(async () => {
  clearRetry();
  // A retry from the offline screen keeps that screen up instead of flashing "Connecting".
  if (session.get().phase !== 'offline') session.set({ phase: 'starting', problem: null });
  try {
    await startTelemetry();
    const [prefs, key, token] = await Promise.all([loadPrefs(), accountKey.get(), relayToken.get()]);
    hasKey = !!key;
    session.set({ prefs });
    let st = await Ursa.init(key, prefs.relays, token);
    for (let i = 1; i <= 2 && hasKey && !st.hasAccount && offlineTries === 0; i++) {
      await sleep(1500 * i);
      st = await Ursa.init(key, prefs.relays, token);
    }
    if (st.openError) log('WARN', `start: the account did not open: ${st.openError}`);
    apply(st);
  } catch (e) {
    session.set({ phase: 'failed', status: null, problem: errorText(e) });
    log('ERROR', `start failed: ${errorText(e)}`);
  }
});

/** Re-reads the status the core holds (after a change this phone made). */
export async function refreshStatus() {
  apply(await Ursa.status());
}

/** Re-reads the account from its nodes (what other devices changed). Throws when none answer. */
export async function syncAccount() {
  apply(await Ursa.refresh());
}

/** Keeps an account this phone has just created, recovered or joined. */
export async function adopt(keys: AccountKeys) {
  await accountKey.set(keys.accountKey);
  hasKey = true;
  // A link hands over the other device's relays and token, and the core already runs on them:
  // keep them for the next start (and for the Network screen).
  if (keys.relays) {
    await setPrefs({ relays: keys.relays });
    await relayToken.set(keys.relayToken ?? null);
  }
  await refreshStatus();
}

/** Forgets the account on this phone. The library and upload queue empty themselves on `welcome`. */
export async function signOut() {
  await Ursa.signOut();
  await accountKey.clear();
  hasKey = false;
  await refreshStatus();
}

export async function setPrefs(p: Partial<Prefs>) {
  const next = { ...session.get().prefs, ...p };
  session.set({ prefs: next });
  await savePrefs(next);
}

/** Saves other relays and restarts the core on them. */
export async function setNetwork(relays: string[], token: string | null) {
  await setPrefs({ relays });
  await relayToken.set(token);
  await Ursa.shutdown();
  session.set({ phase: 'starting', status: null, problem: null });
  offlineTries = 0;
  await start();
}
