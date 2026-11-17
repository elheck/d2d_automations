// Rules and texts of the inventory import page (tested without rendering).

import type { SalesPaceView, UnusualSalesView } from "../api/types";

/** Largest export the server takes (it checks again). */
export const MAX_IMPORT_BYTES = 16 * 1024 * 1024;

/** Why a chosen file cannot be checked, or `null`. */
export function importFileProblem(file: { name: string; size: number }): string | null {
  if (file.size > MAX_IMPORT_BYTES) {
    return `The file is too large (at most ${MAX_IMPORT_BYTES / (1024 * 1024)} MB).`;
  }
  if (file.size === 0) return "The file is empty.";
  return null;
}

/** `"1,234"`, `"1.234"`, `" 1234 "` → 1234; like the server, anything else `null`. */
export function typedCount(text: string): number | null {
  const digits = text.replace(/[,.' \u00a0\u202f]/g, "");
  if (!/^[0-9]+$/.test(digits)) return null;
  return Number(digits);
}

export const KIND_LABELS: Record<string, string> = {
  import: "Import",
  forced_import: "Import (blocked, forced)",
  restore: "Restored from a backup",
  desktop_copy: "Copy of the desktop data",
};

/** Error codes after which checking the same file again helps. */
export const CHECK_AGAIN_CODES = new Set(["no_preview", "preview_expired", "outdated", "changed", "other_file"]);

function copies(n: number): string {
  return `${Math.round(n).toLocaleString("en-US")} ${Math.round(n) === 1 ? "copy" : "copies"}`;
}

function days(n: number): string {
  return `${n} ${n === 1 ? "day" : "days"}`;
}

/** "Usually about 21 copies sell per day (over the 30 days up to 2026-11-11)." */
export function paceText(pace: SalesPaceView | null): string {
  if (!pace) return "There is not enough sales history yet to compare with (needs a week of imports).";
  const daily = pace.daily_copies < 10 ? pace.daily_copies.toFixed(1) : Math.round(pace.daily_copies).toString();
  return `Usually about ${daily} copies sell per day (over the ${days(pace.days)} up to ${pace.last_import}).`;
}

/** The warning for clearly more sales than usual. */
export function unusualText(unusual: UnusualSalesView, copiesSold: number): string {
  return (
    `${copies(copiesSold)} would count as sold: more than 3× the usual. ` +
    `The last import was ${days(unusual.days_since_last_import)} ago; about ${copies(unusual.expected_copies)} ` +
    `would be expected. Check that the file is complete and is the right export before you import it.`
  );
}
