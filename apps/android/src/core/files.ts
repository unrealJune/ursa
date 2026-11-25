import * as Sharing from 'expo-sharing';
import { type LibraryItem, Ursa, withProgress } from 'ursa-core';

/** Downloads a file into the cache (once per file) and returns its file:// URI. */
export function fetchFile(item: LibraryItem, onProgress?: (fraction: number) => void): Promise<string> {
  return withProgress(
    (task) => Ursa.download(item.fileId, item.name, task),
    (done, total) => onProgress?.(total > 0 ? done / total : 0),
  );
}

export async function shareFile(item: LibraryItem, onProgress?: (fraction: number) => void) {
  const uri = await fetchFile(item, onProgress);
  await Sharing.shareAsync(uri, { mimeType: item.mime, dialogTitle: item.name });
}

export async function openFile(item: LibraryItem, onProgress?: (fraction: number) => void) {
  Ursa.open(await fetchFile(item, onProgress), item.mime);
}

/** Saves into the phone's gallery (photos, videos) or Downloads (everything else). */
export async function saveFile(item: LibraryItem, onProgress?: (fraction: number) => void) {
  await Ursa.saveToDevice(await fetchFile(item, onProgress), item.name, item.mime);
}
