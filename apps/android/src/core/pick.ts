import * as DocumentPicker from 'expo-document-picker';
import * as ImagePicker from 'expo-image-picker';

import type { QueueItem } from './store';

type Enqueue = (files: QueueItem[]) => void;

/** Android's photo picker, several at once. */
export async function pickAndUpload(enqueue: Enqueue) {
  const r = await ImagePicker.launchImageLibraryAsync({ mediaTypes: ['images', 'videos'], allowsMultipleSelection: true, quality: 1, exif: false });
  if (r.canceled) return;
  enqueue(
    r.assets.map((a) => ({
      uri: a.uri,
      meta: { name: a.fileName ?? undefined, mime: a.mimeType ?? undefined, width: a.width || undefined, height: a.height || undefined },
    })),
  );
}

/** Any files, through the system's document picker. */
export async function pickFiles(enqueue: Enqueue) {
  const r = await DocumentPicker.getDocumentAsync({ multiple: true, copyToCacheDirectory: false });
  if (r.canceled) return;
  enqueue(r.assets.map((a) => ({ uri: a.uri, meta: { name: a.name, mime: a.mimeType ?? undefined } })));
}
