import { AppState } from 'react-native';

import { libraryOpened, resetLibrary, syncIfStale } from './library';
import { session, start } from './session';
import { watch } from './state';
import { resumeStorage } from './storage';
import { forgetThumbs } from './thumbs';
import { backupNow, commitUploads, resetUploads } from './uploads';

/**
 * While the app is on screen, re-read the account from its nodes this often, so a change made on
 * another device (a delete on Windows) shows up without a pull or a restart. A sync that finds
 * nothing new costs one small head read per node.
 */
const SYNC_EVERY_MS = 30_000;

let booted = false;

/**
 * Starts the core once per process and ties the stores together: the library opens with the
 * account and empties when it goes, backup runs while the app is open, and an offline phone
 * retries as soon as it is back on screen.
 */
export function boot() {
  if (booted) return;
  booted = true;

  watch(
    session,
    (s) => s.phase,
    (phase, prev) => {
      if (phase === 'ready' && prev !== 'ready') {
        libraryOpened();
        if (session.get().prefs.backup) backupNow();
        resumeStorage();
      }
      if (phase === 'welcome') {
        resetLibrary();
        resetUploads();
        forgetThumbs();
      }
    },
  );

  watch(
    session,
    (s) => s.prefs.backup,
    (on) => {
      if (on && session.get().phase === 'ready') backupNow();
    },
  );

  AppState.addEventListener('change', (state) => {
    // Android may end a process that is not on screen: write the staged uploads first.
    if (state === 'background') {
      commitUploads();
      return;
    }
    if (state !== 'active') return;
    const { phase, prefs } = session.get();
    if (phase === 'offline') start();
    if (phase === 'ready') {
      syncIfStale(SYNC_EVERY_MS);
      if (prefs.backup) backupNow();
    }
  });

  setInterval(() => {
    if (AppState.currentState === 'active' && session.get().phase === 'ready') syncIfStale(SYNC_EVERY_MS - 1000);
  }, SYNC_EVERY_MS);

  start();
}
