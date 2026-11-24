import { useSyncExternalStore } from 'react';

/**
 * A small external store. State lives outside React, so actions are plain functions any module
 * can call (no stale closures over a provider's state), and a component re-renders only when the
 * slice it selects changes, not on every upload progress tick.
 */
export type Store<T extends object> = {
  get(): T;
  set(patch: Partial<T> | ((s: T) => Partial<T>)): void;
  subscribe(listener: () => void): () => void;
};

export function createStore<T extends object>(initial: T): Store<T> {
  let state = initial;
  const listeners = new Set<() => void>();
  const store: Store<T> = {
    get: () => state,
    set(patch) {
      const p = typeof patch === 'function' ? patch(state) : patch;
      let changed = false;
      for (const k in p) {
        if (!Object.is(p[k], state[k])) {
          changed = true;
          break;
        }
      }
      if (!changed) return;
      state = { ...state, ...p };
      listeners.forEach((l) => l());
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return store;
}

/**
 * Reads a slice of a store and re-renders when it changes. `select` must return state it reads
 * (or a primitive), not a fresh object or array.
 *
 * A standalone hook, not a `store.use(...)` method: the React Compiler takes any call named `use`
 * for React's own `use()`, which may be called conditionally, and compiled the store's hook into
 * a conditional call, so the Account tab crashed with "Cannot read property 'next' of undefined".
 */
export function useStore<T extends object, U>(store: Store<T>, select: (s: T) => U): U {
  return useSyncExternalStore(store.subscribe, () => select(store.get()));
}

/** Calls `fn` with each new value `select` returns (not the current one). */
export function watch<T extends object, U>(store: Store<T>, select: (s: T) => U, fn: (next: U, prev: U) => void): () => void {
  let prev = select(store.get());
  return store.subscribe(() => {
    const next = select(store.get());
    if (Object.is(next, prev)) return;
    const was = prev;
    prev = next;
    fn(next, was);
  });
}

/** One run at a time: a call while one is running joins it. */
export function singleFlight<A extends unknown[]>(run: (...args: A) => Promise<void>): (...args: A) => Promise<void> {
  let running: Promise<void> | null = null;
  return (...args: A) => {
    if (!running) running = run(...args).finally(() => (running = null));
    return running;
  };
}

export const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
