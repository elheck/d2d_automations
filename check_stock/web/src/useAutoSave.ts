import { useEffect, useRef } from "react";
import { useDebounced } from "./useDebounced";

/** Wait after the last change before settings are saved. */
export const SAVE_DELAY_MS = 800;

/**
 * Saves `value` once it stopped changing for {@link SAVE_DELAY_MS}, and right
 * away when the page is left with changes not saved yet: leaving to another
 * page of the app uses `save` (a normal request that updates the cache); a
 * reload or closed tab uses `saveOnExit` (a `keepalive` request, because the
 * page just goes away without unmounting). `null` = invalid, nothing is saved;
 * `stored` is what the server has. Values are compared by their JSON.
 */
export function useAutoSave<T>(value: T | null, stored: T, save: (value: T) => void, saveOnExit: (value: T) => void) {
  const savedKey = useRef(JSON.stringify(stored));
  const latest = useRef(value);
  const settled = useDebounced(value, SAVE_DELAY_MS);
  // The newest callbacks, without restarting the effects below.
  const callbacks = useRef({ save, saveOnExit });

  useEffect(() => {
    latest.current = value;
    callbacks.current = { save, saveOnExit };
  });

  useEffect(() => {
    if (settled === null) return;
    const key = JSON.stringify(settled);
    if (key === savedKey.current) return;
    savedKey.current = key;
    callbacks.current.save(settled);
  }, [settled]);

  useEffect(() => {
    /** The value not saved yet, marked as saved. */
    const takeUnsaved = () => {
      const pending = latest.current;
      if (pending === null || JSON.stringify(pending) === savedKey.current) return null;
      savedKey.current = JSON.stringify(pending);
      return pending;
    };
    const onPageHide = () => {
      const pending = takeUnsaved();
      if (pending !== null) callbacks.current.saveOnExit(pending);
    };
    window.addEventListener("pagehide", onPageHide);
    return () => {
      window.removeEventListener("pagehide", onPageHide);
      const pending = takeUnsaved();
      if (pending !== null) callbacks.current.save(pending);
    };
  }, []);
}
