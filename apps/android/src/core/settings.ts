import * as SecureStore from 'expo-secure-store';

/** Street Cryptid's relays, the same default the Windows app has (n0's public relays carried almost nothing). */
export const DEFAULT_RELAYS = ['https://iroh-euw.streetcrypt.id', 'https://iroh-wus.streetcrypt.id'];

const ACCOUNT = 'ursa.account';
const RELAY_TOKEN = 'ursa.relay-token';
const PREFS = 'ursa.prefs';

export type Prefs = {
  relays: string[];
  /** Back up the camera roll while the app is open. */
  backup: boolean;
  /** Back up videos too. */
  backupVideos: boolean;
  /** How far the photo grid is zoomed: an index into the grid's zoom levels. */
  gridZoom: number;
};

export const DEFAULT_PREFS: Prefs = { relays: DEFAULT_RELAYS, backup: false, backupVideos: true, gridZoom: 2 };

/** The account root (hex), kept by the Android Keystore through expo-secure-store. */
export const accountKey = {
  get: () => SecureStore.getItemAsync(ACCOUNT),
  set: (hex: string) => SecureStore.setItemAsync(ACCOUNT, hex),
  clear: () => SecureStore.deleteItemAsync(ACCOUNT),
};

export const relayToken = {
  get: () => SecureStore.getItemAsync(RELAY_TOKEN),
  set: (t: string | null) => (t ? SecureStore.setItemAsync(RELAY_TOKEN, t) : SecureStore.deleteItemAsync(RELAY_TOKEN)),
};

export async function loadPrefs(): Promise<Prefs> {
  const raw = await SecureStore.getItemAsync(PREFS);
  if (!raw) return DEFAULT_PREFS;
  try {
    return { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<Prefs>) };
  } catch {
    return DEFAULT_PREFS;
  }
}

export async function savePrefs(p: Prefs): Promise<void> {
  await SecureStore.setItemAsync(PREFS, JSON.stringify(p));
}
