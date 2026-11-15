// Helpers of the singles listing: the quick card entry and price texts.
// The server applies the same input rules (check_stock_core::card_input);
// here they only show a preview and avoid requests that would be refused.

import type { PriceLookup, PriceSet } from "../api/types";

export interface CardRef {
  /** Lower-case set code, e.g. `hou`. */
  setCode: string;
  /** Collector number without leading zeros, e.g. `5` for `005`. */
  collectorNumber: string;
}

const SAFE_SET = /^[a-z0-9-]{1,10}$/;

function stripLeadingZeros(digits: string): string {
  const stripped = digits.replace(/^0+/, "");
  return stripped === "" ? "0" : stripped;
}

/** `hou120` → set `hou`, number `120`: the last three characters are digits. */
export function parseCardInput(input: string): CardRef | null {
  const text = input.trim().toLowerCase();
  if (text.length < 4) return null;
  const setCode = text.slice(0, -3);
  const number = text.slice(-3);
  if (!/^\d{3}$/.test(number) || !SAFE_SET.test(setCode)) return null;
  return { setCode, collectorNumber: stripLeadingZeros(number) };
}

/** Like {@link parseCardInput}; exactly three digits use `defaultSet`. */
export function resolveCardInput(input: string, defaultSet: string): CardRef | null {
  const text = input.trim();
  const set = defaultSet.trim().toLowerCase();
  if (/^\d{3}$/.test(text) && set !== "") {
    return SAFE_SET.test(set) ? { setCode: set, collectorNumber: stripLeadingZeros(text) } : null;
  }
  return parseCardInput(text);
}

export const PRICE_ROWS: { key: keyof PriceSet; label: string }[] = [
  { key: "trend", label: "Trend" },
  { key: "low", label: "Low" },
  { key: "avg", label: "Average" },
  { key: "avg1", label: "Avg (1 day)" },
  { key: "avg7", label: "Avg (7 days)" },
  { key: "avg30", label: "Avg (30 days)" },
];

/** Why there are no Cardmarket prices, in words. */
export function priceStatusText(prices: Exclude<PriceLookup, { status: "found" }>): string {
  switch (prices.status) {
    case "no_cardmarket_id":
      return "Scryfall knows no Cardmarket product for this printing.";
    case "not_tracked":
      return "The price tracker has no prices for this product.";
    case "not_configured":
      return "No price tracker is configured on the server.";
    case "unavailable":
      return "The price tracker did not answer. Try again in a moment.";
  }
}

/** Only Cardmarket links are shown as links (the server builds them). */
export function isCardmarketUrl(url: string | null): url is string {
  return url !== null && url.startsWith("https://www.cardmarket.com/");
}

export const DEFAULT_SET_KEY = "check_stock.singles.default_set";

/** The default set remembered on this device ("" if none or unreadable). */
export function loadDefaultSet(): string {
  try {
    return window.localStorage.getItem(DEFAULT_SET_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveDefaultSet(set: string): void {
  try {
    if (set) window.localStorage.setItem(DEFAULT_SET_KEY, set);
    else window.localStorage.removeItem(DEFAULT_SET_KEY);
  } catch {
    // Private mode or blocked storage: the default set is only a convenience.
  }
}
