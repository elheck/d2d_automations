// Rules and texts of the bin analysis and move list pages (tested without
// rendering).

import type { MoveListKind, MoveView, SourceInfo } from "../api/types";

/** A whole number from `min` to `max`, or `null`. */
export function parseWhole(text: string, min: number, max: number): number | null {
  if (!/^\s*\d{1,3}\s*$/.test(text)) return null;
  const value = Number(text);
  return value >= min && value <= max ? value : null;
}

export const KIND_LABELS: Record<MoveListKind, string> = {
  consolidation: "Empty sparse bins",
  defrag: "Gather spread cards",
};

/** "From the import of 15.11.2026, 10:00 (inventory-report-Magic.csv)". */
export function sourceText(source: SourceInfo): string {
  return `From the inventory import of ${new Date(source.imported_at).toLocaleString()} (${source.file_name}).`;
}

export interface BinGroup {
  bin: string;
  moves: MoveView[];
  cards: number;
  done: number;
}

/** Moves grouped by the bin they come from (in the server's order). */
export function groupBySource(moves: MoveView[]): BinGroup[] {
  const groups = new Map<string, BinGroup>();
  for (const m of moves) {
    let group = groups.get(m.from_bin);
    if (!group) {
      group = { bin: m.from_bin, moves: [], cards: 0, done: 0 };
      groups.set(m.from_bin, group);
    }
    group.moves.push(m);
    group.cards += m.quantity;
    if (m.done) group.done += 1;
  }
  return [...groups.values()];
}

/** File name of the "moved" CSV of a list kind. */
export function exportName(kind: MoveListKind): string {
  return kind === "consolidation" ? "bin_consolidation_moved.csv" : "bin_defrag_moved.csv";
}

/** "1 pile", "3 piles". */
export function count(n: number, word: string): string {
  return `${n.toLocaleString("en-US")} ${word}${n === 1 ? "" : "s"}`;
}
