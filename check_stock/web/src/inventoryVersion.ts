// The inventory version this browser tab shows. When the server reports a
// newer one (an import by someone), the layout offers to load the new data;
// after the user's own import the page marks the new version as seen.

import { useSyncExternalStore } from "react";

let seen: number | null = null;
const listeners = new Set<() => void>();

export function markSeen(version: number): void {
  if (seen !== null && version <= seen) return;
  seen = version;
  listeners.forEach((listener) => listener());
}

export function useSeenVersion(): number | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => seen,
  );
}

/** Tests start from nothing seen. */
export function resetSeenVersion(): void {
  seen = null;
  listeners.forEach((listener) => listener());
}
