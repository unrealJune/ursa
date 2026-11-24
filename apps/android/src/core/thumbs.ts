import { useEffect, useState } from 'react';
import { Ursa } from 'ursa-core';

/** Decrypted thumbnails, fetched at most six at a time and remembered for the session. */
const cache = new Map<string, Promise<string | null>>();
const waiting: (() => void)[] = [];
let running = 0;
const MAX = 6;

function slot(): Promise<void> {
  if (running < MAX) {
    running += 1;
    return Promise.resolve();
  }
  return new Promise((resolve) => waiting.push(resolve));
}

function release() {
  const next = waiting.shift();
  if (next) next();
  else running -= 1;
}

export function thumb(fileId: string): Promise<string | null> {
  let p = cache.get(fileId);
  if (!p) {
    p = (async () => {
      await slot();
      try {
        return await Ursa.thumb(fileId);
      } catch {
        cache.delete(fileId); // try again next time it is on screen
        return null;
      } finally {
        release();
      }
    })();
    cache.set(fileId, p);
  }
  return p;
}

export function useThumb(fileId: string, hasThumb: boolean): string | null {
  const [uri, setUri] = useState<string | null>(null);
  useEffect(() => {
    if (!hasThumb) return;
    let live = true;
    thumb(fileId).then((u) => live && setUri(u));
    return () => {
      live = false;
    };
  }, [fileId, hasThumb]);
  return uri;
}

export function forgetThumbs() {
  cache.clear();
}
