// Server answers for the precon EV tests.

import type {
  CardLine,
  EvHistory,
  JobStatus,
  PreconSettings,
  PreconSettingsView,
  PreconStatusView,
  ProductDetail,
  RankingRow,
  RankingView,
} from "../api/types";

export const ARMY = 774134;
export const FAMILY = 774135;

export const preconDefaults: PreconSettings = {
  field: "trend",
  sealed_field: "trend",
  cutoff: 0.5,
  foil_needs_sales: true,
  sort: "ratio",
  desc: true,
  hide_no_decklist: false,
};

export function preconSettings(settings: Partial<PreconSettings> = {}): PreconSettingsView {
  return { settings: { ...preconDefaults, ...settings }, defaults: preconDefaults };
}

export const idle: JobStatus = { state: "idle" };
export const finished: JobStatus = { state: "finished", at: "2026-11-15T08:00:00Z" };

export function status(overrides: Partial<PreconStatusView> = {}): PreconStatusView {
  return {
    configured: true,
    load: finished,
    sales: finished,
    loaded: {
      version: 2,
      loaded_at: "2026-11-15T08:00:00Z",
      products: 2,
      with_decklist: 1,
      deck_list_version: "5.2.2+20261114",
      index_date: "2026-11-10",
      index_stale: false,
      catalog_truncated: false,
      sales_loaded: true,
      sales_error: null,
    },
    ...overrides,
  };
}

export function row(overrides: Partial<RankingRow> = {}): RankingRow {
  return {
    id_product: ARMY,
    name: 'Commander: Bloomburrow: "Animated Army" Commander Deck',
    set_label: "Bloomburrow Commander",
    release_date: "2024-08-02",
    state: "ready",
    decks: 1,
    sealed_price: 40,
    sealed_low: 38,
    stale_sealed: null,
    ev: {
      ev: 52.5,
      gross: 61.3,
      top5: 30,
      liquid: 40.25,
      coverage: 0.98,
      total_copies: 100,
      priced_copies: 98,
      counted_copies: 40,
      below_cutoff_copies: 58,
      unpriced_copies: 2,
      foil_fallback_copies: 0,
    },
    ratio: 1.3125,
    note: "2 unpriced",
    note_detail: null,
    cardmarket_url: `https://www.cardmarket.com/en/Magic/Products?idProduct=${ARMY}`,
    ...overrides,
  };
}

export function ranking(
  rows: RankingRow[] = [
    row(),
    row({
      id_product: FAMILY,
      name: "Family Matters",
      state: "not_on_mtgjson",
      decks: 0,
      ev: null,
      ratio: null,
      note: "not on MTGJSON yet",
      sealed_price: 30,
    }),
  ],
): RankingView {
  return { version: 2, rows, total: 2, ready: 1 };
}

export function card(overrides: Partial<CardLine> = {}): CardLine {
  return {
    rank: 1,
    top5: true,
    name: "Sol Ring",
    is_foil: false,
    set_code: "blc",
    number: "296",
    board: "main",
    count: 1,
    unit: 2.5,
    foil_fallback: false,
    line: 2.5,
    counted: true,
    sale_days: 9,
    observed_days: 14,
    liquidity: 1,
    note: "",
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=101",
    ...overrides,
  };
}

export function product(): ProductDetail {
  return {
    version: 2,
    row: row(),
    decks: [
      {
        name: "Animated Army",
        ev: 52.5,
        cards: [
          card(),
          card({
            rank: 2,
            top5: false,
            name: "Cheap Elf",
            is_foil: true,
            unit: 0.1,
            line: 0.1,
            counted: false,
            // Under 0.50 €: not checked for sales (0 of 0 days).
            sale_days: 0,
            observed_days: 0,
            liquidity: null,
            note: "below cutoff",
            cardmarket_url: null,
          }),
        ],
      },
    ],
  };
}

export function history(): EvHistory {
  return {
    version: 2,
    points: [
      { date: "2026-11-01", ev: 50, top5: 28, sealed: 42, coverage: 0.97 },
      { date: "2026-11-08", ev: null, top5: null, sealed: 41, coverage: 0.5 },
      { date: "2026-11-15", ev: 52.5, top5: 30, sealed: 40, coverage: 0.98 },
    ],
  };
}
