// Rules and texts of the Search page (tested without rendering).

import type { PileRow } from "../api/types";

/** The order steps after a discard (as in the desktop app). */
export const DISCARD_STEPS = [
  "Discard (next): the CSV downloads right away.",
  "Import it into TCG Powertools right away, so Cardmarket drops the cards too.",
  "Check the cards are gone on Cardmarket, then download a fresh inventory export.",
  "Import that fresh export here (Tools → Inventory import).",
];

/** A pile in the selection. */
export interface Selected {
  pile: PileRow;
  quantity: number;
}

/** Adds `quantity` copies of `pile` (at most what the pile has). */
export function addToSelection(selection: Selected[], pile: PileRow, quantity: number): Selected[] {
  const existing = selection.find((s) => s.pile.id === pile.id);
  if (existing) {
    return selection.map((s) =>
      s.pile.id === pile.id ? { ...s, quantity: Math.min(pile.quantity, s.quantity + quantity) } : s,
    );
  }
  return [...selection, { pile, quantity: Math.min(pile.quantity, quantity) }];
}

/** Copies and listed value of a selection. */
export function selectionTotals(selection: Selected[]): { copies: number; value: number } {
  return selection.reduce((t, s) => ({ copies: t.copies + s.quantity, value: t.value + s.quantity * s.pile.price }), {
    copies: 0,
    value: 0,
  });
}

/** `+12.3%` / `−4.0%` / `—`. */
export function formatPct(value: number | null): string {
  if (value === null) return "—";
  return `${value >= 0 ? "+" : "−"}${Math.abs(value).toFixed(1)}%`;
}
