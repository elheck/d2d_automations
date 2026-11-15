// Rules and texts of the Stock Checker and Picking pages (tested without
// rendering), and the hand-over of a Search selection to the Stock Checker.

import type { CheckGroup, OutputGroup, PickItemView, PileRow } from "../api/types";

/** Largest list the server takes (it checks again). */
export const MAX_WANTSLIST_BYTES = 256 * 1024;

export const LANGUAGES = ["English", "German", "Spanish", "French", "Italian"] as const;

/** Why a chosen file cannot be used, or `null`. */
export function wantslistFileProblem(file: { size: number }): string | null {
  if (file.size > MAX_WANTSLIST_BYTES) return "The file is too large (at most 256 KB).";
  if (file.size === 0) return "The file is empty.";
  return null;
}

/** A pile chosen in Search, waiting for the Stock Checker (memory only). */
let handoff: { pile: PileRow; quantity: number }[] | null = null;

export function handOver(selection: { pile: PileRow; quantity: number }[]): void {
  handoff = selection;
}

/** The Search selection as check groups (one per card name), once. */
export function takeHandoff(): CheckGroup[] | null {
  const taken = handoff;
  handoff = null;
  if (!taken || taken.length === 0) return null;
  const groups = new Map<string, CheckGroup>();
  for (const { pile, quantity } of taken) {
    let group = groups.get(pile.name);
    if (!group) {
      group = { name: pile.name, wanted: 0, found: 0, piles: [] };
      groups.set(pile.name, group);
    }
    group.wanted += quantity;
    group.found += quantity;
    group.piles.push({ pile, take: quantity });
  }
  return [...groups.values()];
}

/** Key of a matched pile within a check (a pile can match one card only). */
export const pileKey = (group: number, pile: string) => `${group}:${pile}`;

/** Every matched pile, chosen. */
export function allChosen(groups: CheckGroup[]): Set<string> {
  const keys = new Set<string>();
  groups.forEach((g, i) => g.piles.forEach((p) => keys.add(pileKey(i, p.pile.id))));
  return keys;
}

/** The chosen piles per card, as the server takes them for outputs. */
export function chosenGroups(groups: CheckGroup[], chosen: Set<string>): OutputGroup[] {
  return groups
    .map((g, i) => ({
      name: g.name,
      wanted: g.wanted,
      items: g.piles
        .filter((p) => chosen.has(pileKey(i, p.pile.id)))
        .map((p) => ({ pile: p.pile.id, quantity: p.take })),
    }))
    .filter((g) => g.items.length > 0);
}

/** Picked and total copies and listed values of a picking list. */
export function pickingTotals(items: PickItemView[]) {
  let picked = 0;
  let total = 0;
  let pickedValue = 0;
  let totalValue = 0;
  for (const i of items) {
    total += 1;
    totalValue += i.price * i.quantity;
    if (i.picked) {
      picked += 1;
      pickedValue += i.price * i.quantity;
    }
  }
  return { picked, total, pickedValue, totalValue };
}
