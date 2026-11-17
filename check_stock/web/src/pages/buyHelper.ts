// Helpers of the buy helper: the settings form and the card list file.
// The server checks the same limits again (server/src/api/buy_helper.rs).

import type { Download } from "../api/client";
import type { BuyOfferParams } from "../api/types";

/** Largest card list accepted (the server's `MAX_CSV_BYTES`). */
export const MAX_FILE_BYTES = 6 * 1024 * 1024;
/** Largest money amount and batch size the server accepts. */
export const MAX_AMOUNT = 1_000_000;
export const MAX_BATCH = 1_000_000;

/** The settings as typed: numbers stay text until they are valid. */
export interface SettingsForm {
  singleCommon: boolean;
  singleUncommon: boolean;
  singleRare: boolean;
  singleMythic: boolean;
  useMinPrice: boolean;
  minPrice: string;
  percent: string;
  bulkRate: string;
  bulkBatch: string;
}

export type FormField = "minPrice" | "percent" | "bulkRate" | "bulkBatch";

export type FormCheck =
  { params: BuyOfferParams; errors?: undefined } | { params?: undefined; errors: Partial<Record<FormField, string>> };

/** The form for `params`; an unused price rule shows the default price. */
export function formFromParams(params: BuyOfferParams, defaults: BuyOfferParams): SettingsForm {
  return {
    singleCommon: params.single_common,
    singleUncommon: params.single_uncommon,
    singleRare: params.single_rare,
    singleMythic: params.single_mythic,
    useMinPrice: params.min_single_price !== null,
    minPrice: String(params.min_single_price ?? defaults.min_single_price ?? 0.5),
    percent: String(params.single_buy_percent),
    bulkRate: String(params.bulk_rate),
    bulkBatch: String(params.bulk_batch),
  };
}

/** `0,50` and `0.50` both mean half a euro; anything else that is not a plain number is `null`. */
export function parseDecimal(text: string): number | null {
  const normalized = text.trim().replace(",", ".");
  if (!/^\d+(\.\d+)?$/.test(normalized)) return null;
  const value = Number(normalized);
  return Number.isFinite(value) ? value : null;
}

/** The server's settings from the form, or what is wrong with it. */
export function paramsFromForm(form: SettingsForm): FormCheck {
  const errors: Partial<Record<FormField, string>> = {};
  const percent = parseDecimal(form.percent);
  if (percent === null || percent > 100) errors.percent = "Enter a share from 0 to 100.";
  const bulkRate = parseDecimal(form.bulkRate);
  if (bulkRate === null || bulkRate > MAX_AMOUNT) errors.bulkRate = "Enter an amount from 0 to 1,000,000.";
  const bulkBatch = /^\s*\d+\s*$/.test(form.bulkBatch) ? Number(form.bulkBatch) : null;
  if (bulkBatch === null || bulkBatch < 1 || bulkBatch > MAX_BATCH) {
    errors.bulkBatch = "Enter a whole number of cards from 1 to 1,000,000.";
  }
  let minPrice: number | null = null;
  if (form.useMinPrice) {
    minPrice = parseDecimal(form.minPrice);
    if (minPrice === null || minPrice > MAX_AMOUNT) errors.minPrice = "Enter an amount from 0 to 1,000,000.";
  }
  if (Object.keys(errors).length > 0 || percent === null || bulkRate === null || bulkBatch === null) {
    return { errors };
  }
  return {
    params: {
      single_common: form.singleCommon,
      single_uncommon: form.singleUncommon,
      single_rare: form.singleRare,
      single_mythic: form.singleMythic,
      min_single_price: minPrice,
      single_buy_percent: percent,
      bulk_rate: bulkRate,
      bulk_batch: bulkBatch,
    },
  };
}

/** Why a chosen file cannot be used, or `null` if it can. */
export function fileProblem(file: { name: string; size: number }): string | null {
  if (file.size > MAX_FILE_BYTES) {
    return `The file is too large (at most ${MAX_FILE_BYTES / (1024 * 1024)} MB).`;
  }
  if (file.size === 0) return "The file is empty.";
  return null;
}

/** Hands a downloaded file to the browser's "save" (no inline script needed). */
export function saveDownload(file: Download): void {
  const url = URL.createObjectURL(file.blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = file.fileName;
  link.rel = "noopener";
  document.body.appendChild(link);
  link.click();
  link.remove();
  // Give the browser time to start the download before the URL goes away.
  window.setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
