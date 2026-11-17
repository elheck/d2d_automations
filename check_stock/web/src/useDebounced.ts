import { useEffect, useMemo, useState } from "react";

/**
 * `value`, but only once it has stayed the same for `delay` ms. Values are
 * compared by their JSON, so a new object with the same content does not
 * restart the wait (they must be JSON-safe).
 */
export function useDebounced<T>(value: T, delay: number): T {
  const key = JSON.stringify(value) ?? "null";
  const [settled, setSettled] = useState(key);
  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(key), delay);
    return () => window.clearTimeout(timer);
  }, [key, delay]);
  return useMemo(() => JSON.parse(settled) as T, [settled]);
}
