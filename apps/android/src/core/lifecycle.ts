import { AppState } from 'react-native';

import { libraryOpened, resetLibrary, syncIfStale } from './library';
import { session, start } from './session';
import { watch } from './state';
import { forgetThumbs } from './thumbs';
import { backupNow, resetUploads } from './uploads';

/** Re-read the account from its nodes when the app comes back after this long. */
const SYNC_AFTER_MS = 60_000;

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
    if (state !== 'active') return;
    const { phase, prefs } = session.get();
    if (phase === 'offline') start();
    if (phase === 'ready') {
      syncIfStale(SYNC_AFTER_MS);
      if (prefs.backup) backupNow();
    }
  });

  start();
}
