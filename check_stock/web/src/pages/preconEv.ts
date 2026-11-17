// Helpers of the precon EV pages: labels, sorting, texts. The values all
// come from the server (check_stock_core::precon_ev); nothing is computed here.

import type { Board, CardLine, EvSummary, JobStatus, PriceColumn, RankingRow, RankingSort } from "../api/types";

export const PRICE_COLUMNS: { value: PriceColumn; label: string }[] = [
  { value: "trend", label: "Trend" },
  { value: "avg", label: "Average" },
  { value: "low", label: "Low" },
  { value: "avg1", label: "Avg 1-day" },
  { value: "avg7", label: "Avg 7-day" },
  { value: "avg30", label: "Avg 30-day" },
];

/** Cardmarket publishes only these for sealed products. */
export const SEALED_COLUMNS: { value: PriceColumn; label: string }[] = PRICE_COLUMNS.filter((c) =>
  ["trend", "low", "avg"].includes(c.value),
);

export function columnLabel(column: PriceColumn): string {
  return PRICE_COLUMNS.find((c) => c.value === column)?.label ?? column;
}

/** Ranking columns; `sort` = the key to sort by (none: not sortable). */
export const RANKING_COLUMNS: { label: string; sort?: RankingSort; numeric?: boolean }[] = [
  { label: "Product", sort: "name" },
  { label: "Set" },
  { label: "Released", sort: "release" },
  { label: "Sealed", sort: "sealed", numeric: true },
  { label: "EV", sort: "ev", numeric: true },
  { label: "EV/Sealed", sort: "ratio", numeric: true },
  { label: "Liquid EV", sort: "liquid", numeric: true },
  { label: "Top 5", sort: "top5", numeric: true },
  { label: "Priced", sort: "coverage", numeric: true },
  { label: "Notes" },
];

/** Clicking a column: the same one flips the direction; another one starts descending (names ascending). */
export function nextSort(
  current: { sort: RankingSort; desc: boolean },
  column: RankingSort,
): { sort: RankingSort; desc: boolean } {
  if (current.sort === column) return { sort: column, desc: !current.desc };
  return { sort: column, desc: column !== "name" };
}

/** `0.25` → `0.25×`. */
export function formatRatio(ratio: number | null): string {
  return ratio === null ? "—" : `${ratio.toFixed(2)}×`;
}

/** How to show the ratio: good (≥ 1), stale (the sealed price is not real), or plain. */
export function ratioTone(row: Pick<RankingRow, "ratio" | "stale_sealed">): "good" | "stale" | "plain" {
  if (row.ratio === null) return "plain";
  if (row.stale_sealed !== null) return "stale";
  return row.ratio >= 1 ? "good" : "plain";
}

/** Share of priced copies as `97%`. */
export function coverageText(ev: Pick<EvSummary, "coverage"> | null): string {
  return ev ? `${Math.round(ev.coverage * 100)}%` : "—";
}

/** Coverage under 95 % deserves a second look. */
export function lowCoverage(ev: Pick<EvSummary, "coverage"> | null): boolean {
  return ev !== null && ev.coverage < 0.95;
}

/** `5/14 d`, or why there is no number. */
export function soldText(card: Pick<CardLine, "sale_days" | "observed_days">): string {
  if (card.sale_days === null || card.observed_days === null) return "—";
  if (card.observed_days === 0) return "not checked";
  return `${card.sale_days}/${card.observed_days} d`;
}

export function soldTitle(card: Pick<CardLine, "sale_days" | "observed_days" | "liquidity">): string | undefined {
  if (card.sale_days === null || card.observed_days === null) return undefined;
  if (card.observed_days === 0) return "Under 0.50 €: not checked, counted as unsellable";
  const weight = Math.round((card.liquidity ?? 0) * 100);
  return `Sold on ${card.sale_days} of ${card.observed_days} observed days (last 14, any seller) · counts ${weight}% in Liquid EV`;
}

export const BOARD_LABELS: Record<Board, string> = {
  commander: "Commander",
  main: "Main",
  side: "Side",
  display: "Display",
};

/** A running job's stage with its counts, e.g. `Loading decklists… 40/120`. */
export function jobText(status: JobStatus): string {
  if (status.state !== "running") return "";
  const counts = status.done !== null && status.total !== null ? ` ${status.done}/${status.total}` : "";
  return `${status.stage}${counts}`;
}

/** Only links to Cardmarket product pages are rendered as links. */
export function isCardmarketUrl(url: string | null | undefined): url is string {
  return typeof url === "string" && url.startsWith("https://www.cardmarket.com/");
}
