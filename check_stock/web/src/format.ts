// Number formats shared by the pages.

/** `0.25` → `0.25 €`; no price → `—`. */
export function formatEuro(value: number | null): string {
  return value === null ? "—" : `${value.toFixed(2)} €`;
}

/** `1004` → `1,004` (counts of cards). */
export function formatCount(value: number): string {
  return value.toLocaleString("en-US");
}
