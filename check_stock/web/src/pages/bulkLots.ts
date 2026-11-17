// Rules and texts of the Bulk Lots page (tested without rendering).

import type { CardTypeKey, FoilKey, LotLine, LotStatus, OrderKey, RarityKey } from "../api/types";

export const RARITIES: { key: RarityKey; label: string }[] = [
  { key: "common", label: "Common" },
  { key: "uncommon", label: "Uncommon" },
  { key: "rare", label: "Rare" },
  { key: "mythic", label: "Mythic" },
  { key: "other", label: "Other (tokens, lands, specials)" },
];

export const CARD_TYPES: { key: CardTypeKey; label: string }[] = [
  { key: "any", label: "Any type" },
  { key: "non_basic_land", label: "Non-basic lands" },
  { key: "creature", label: "Creatures" },
  { key: "instant_sorcery", label: "Instants & sorceries" },
  { key: "artifact", label: "Artifacts" },
  { key: "enchantment", label: "Enchantments" },
  { key: "planeswalker", label: "Planeswalkers" },
  { key: "non_land", label: "Non-land cards" },
];

export const FOILS: { key: FoilKey; label: string }[] = [
  { key: "non_foil", label: "Non-foil" },
  { key: "foil_only", label: "Foil" },
  { key: "any", label: "Both" },
];

export const ORDERS: { key: OrderKey; label: string }[] = [
  { key: "nearby", label: "Closest together" },
  { key: "oldest_first", label: "Oldest first" },
  { key: "cheapest_first", label: "Cheapest first" },
  { key: "random", label: "Random" },
];

export const STATUS_LABELS: Record<LotStatus, string> = {
  awaiting_import: "Awaiting import",
  overdue: "Awaiting import for over 7 days",
  done: "Done",
};

/** A price typed with comma or point; `null` when it is not one (0–100,000). */
export function parsePrice(text: string): number | null {
  const t = text.trim().replace(",", ".");
  if (!/^\d{1,6}(\.\d{1,2})?$/.test(t)) return null;
  const value = Number(t);
  return value <= 100_000 ? value : null;
}

/** Picked lines and copies of a draft. */
export function pickedTotals(lines: LotLine[]) {
  const picked = lines.filter((l) => l.picked);
  return {
    lines: picked.length,
    copies: picked.reduce((n, l) => n + l.copies, 0),
    listedValue: picked.reduce((v, l) => v + l.copies * l.price, 0),
  };
}

/** Lines whose copies another lot or a discard took meanwhile. */
export function takenLines(lines: LotLine[]): LotLine[] {
  return lines.filter((l) => l.free < l.copies);
}

/** "12 days" / "1 day" / "—". */
export function daysText(days: number | null): string {
  if (days === null) return "—";
  return `${days} ${days === 1 ? "day" : "days"}`;
}
