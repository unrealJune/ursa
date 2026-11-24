import { NativeModule, requireNativeModule } from 'expo';

/** The Ursa core (Rust, through UniFFI) as the app sees it. Errors carry the core's message. */

export type DeviceInfo = { deviceId: string; name: string; platform: string; addedMs: number; thisDevice: boolean };
export type NodeEntry = { id: string; label: string };
export type FriendInfo = { accountId: string; name: string; nodes: number };

export type Status = {
  deviceId: string;
  hasAccount: boolean;
  accountId: string | null;
  name: string | null;
  devices: DeviceInfo[];
  homeNodes: NodeEntry[];
  friends: FriendInfo[];
  /** Set when a stored account could not be opened (its nodes may be offline). */
  openError: string | null;
};

export type AccountKeys = { accountId: string; accountKey: string };

export type LibraryItem = {
  fileId: string;
  name: string;
  size: number;
  mime: string;
  createdMs: number;
  modifiedMs: number;
  addedMs: number;
  width: number | null;
  height: number | null;
  hasThumb: boolean;
};

export type PoolNode = {
  id: string;
  label: string;
  /** `home` for the account's own nodes, else the friend's name. */
  owner: string;
  reachable: boolean;
  path: string | null;
  role: 'owner' | 'granted' | 'none' | null;
  usedBytes: number | null;
  quotaBytes: number | null;
  version: string | null;
  backend: string | null;
  error: string | null;
};

export type RepairReport = { files: number; objectsChecked: number; objectsRebuilt: number; unrecoverable: string[] };

export type UploadMeta = { name?: string; mime?: string; createdMs?: number; modifiedMs?: number; width?: number; height?: number };

type Events = {
  onProgress(e: { task: string; done: number; total: number }): void;
  onLinkCode(e: { code: string }): void;
  onLinkRequest(e: { name: string; platform: string; code: string }): void;
  onLinkConfirm(e: { code: string }): void;
};

declare class UrsaCoreModule extends NativeModule<Events> {
  init(accountKey: string | null, relays: string[], relayToken: string | null): Promise<Status>;
  /** Stops the endpoint, to `init` again with other relays. */
  shutdown(): Promise<void>;
  status(): Promise<Status>;
  /** Re-reads the account from its nodes (what other devices changed), then the status. */
  refresh(): Promise<Status>;
  createAccount(name: string): Promise<AccountKeys>;
  recoverPhrase(phrase: string): Promise<AccountKeys>;
  /** `uri`: the key file, from the document picker. */
  recoverKeyFile(uri: string, passphrase: string): Promise<AccountKeys>;
  linkJoin(code: string): Promise<AccountKeys>;
  /** Resolves with the joined device's name, or null when cancelled. */
  linkOffer(): Promise<string | null>;
  linkDecide(accept: boolean): Promise<void>;
  linkCancel(): Promise<void>;
  pool(): Promise<PoolNode[]>;
  claimNode(link: string, label: string): Promise<string>;
  forgetNode(nodeId: string): Promise<void>;
  invite(): Promise<string>;
  befriend(invite: string, quotaGb: number): Promise<FriendInfo>;
  recoveryPhrase(): Promise<string>;
  /** Writes the key file into the cache and returns its file:// URI, for the share sheet. */
  exportKeyFile(passphrase: string): Promise<string>;
  list(): Promise<LibraryItem[]>;
  thumb(fileId: string): Promise<string>;
  upload(uri: string, meta: UploadMeta, task: string): Promise<LibraryItem>;
  download(fileId: string, name: string, task: string): Promise<string>;
  saveToDevice(fileUri: string, name: string, mime: string): Promise<string>;
  /** Opens a downloaded file in the app that handles its type. */
  open(fileUri: string, mime: string): void;
  delete(fileId: string): Promise<void>;
  repair(task: string): Promise<RepairReport>;
  signOut(): Promise<void>;
}

export const Ursa = requireNativeModule<UrsaCoreModule>('UrsaCore');

let nextTask = 0;
/** A fresh id for a call that reports progress. */
export function taskId(): string {
  nextTask += 1;
  return `t${nextTask}`;
}

/** Runs `run(task)` with `onProgress` called for that task's progress events. */
export async function withProgress<T>(run: (task: string) => Promise<T>, onProgress: (done: number, total: number) => void): Promise<T> {
  const task = taskId();
  const sub = Ursa.addListener('onProgress', (e) => {
    if (e.task === task) onProgress(e.done, e.total);
  });
  try {
    return await run(task);
  } finally {
    sub.remove();
  }
}

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}
