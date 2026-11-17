import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ListingExplained,
  MarketStatusView,
  MeResponse,
  RepricingOptions,
  RepricingSetup,
  ReviewPage,
  ReviewRow,
  Rule,
  Ruleset,
} from "../api/types";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";

const editor: MeResponse = {
  username: "anna",
  display_name: "Anna",
  permissions: ["repricing.view", "repricing.edit_rules", "repricing.export"],
};
const exporter: MeResponse = {
  username: "ben",
  display_name: "Ben",
  permissions: ["repricing.view", "repricing.export"],
};
const viewer: MeResponse = { username: "vic", display_name: "Vic", permissions: ["repricing.view"] };

function rule(name: string, conditions: Rule["conditions"] = []): Rule {
  return {
    name,
    enabled: true,
    conditions,
    action: {
      base: { Market: { field: "Trend", combine: "Only", other: "Avg30" } },
      percent: 100,
      offset: 0,
      adjust: true,
      direction: "Both",
      min_price: null,
      max_price: null,
      max_raise_pct: null,
      max_drop_pct: null,
    },
  };
}

const ruleset: Ruleset = {
  version: 1,
  name: "Shop rules",
  rules: [rule("Protect expensive cards", [{ ListedPrice: { min: 50, max: null } }]), rule("Follow the trend")],
  guardrails: {
    floors: { common: 0.05, uncommon: 0.05, rare: 0.1, mythic: 0.2, other: 0.05 },
    rounding: "Cent",
    min_change_eur: 0.01,
    min_change_pct: 2,
    stale_days: 7,
    review_pct: 30,
    review_min_eur: 1,
    reference: "Trend",
  },
  adjustments: { condition_pct: { NM: 100, EX: 90 }, language_pct: {} },
};

const options: RepricingOptions = {
  rarities: ["Rare", "Common"],
  languages: ["English", "German"],
  conditions: ["NM", "EX", "GD", "LP", "PL", "PO"],
  fields: [
    { key: "Trend", label: "Trend" },
    { key: "Low", label: "Low" },
  ],
  combines: [
    { key: "Only", label: "only" },
    { key: "HigherOf", label: "or higher of" },
  ],
  directions: [
    { key: "Both", label: "Raise and lower" },
    { key: "RaiseOnly", label: "Only raise" },
    { key: "LowerOnly", label: "Only lower" },
  ],
  roundings: [{ key: "Cent", label: "To the cent" }],
  moves: [{ key: "Rising", label: "Rising" }],
  finishes: [
    { key: "NonFoil", label: "Non-foil" },
    { key: "Foil", label: "Foil" },
  ],
  condition_templates: [
    { label: "Rarity", help: "Rarity is one of the picked ones", condition: { Rarity: [] } },
    {
      label: "Listed price",
      help: "Your current price is in a range",
      condition: { ListedPrice: { min: null, max: null } },
    },
  ],
  rule_templates: [{ hint: "Never sell into a dip", rule: rule("Hold falling cards") }],
  sales_window_days: 30,
  min_price: 0.02,
  max_price: 100000,
  max_ruleset_bytes: 1048576,
};

const source = { version: 3, file_name: "inventory-report-Magic.csv", imported_at: "2026-11-01T10:00:00Z" };

function setup(overrides: Partial<RepricingSetup> = {}): RepricingSetup {
  return {
    source,
    published: { revision: 1, ruleset, updated_at: "2026-11-16T10:00:00Z", updated_by: "Anna" },
    draft: null,
    options,
    ...overrides,
  };
}

function row(name: string, overrides: Partial<ReviewRow> = {}): ReviewRow {
  return {
    pile: `pile-${name}`,
    name,
    set_code: "TST",
    set_name: "Test Set",
    collector_number: "1",
    rarity: "Rare",
    condition: "NM",
    language: "English",
    foil: false,
    quantity: 2,
    location: "A-0-0-1-L3-R",
    old_price: 9,
    new_price: 10,
    market: 10,
    outcome: "raise",
    outcome_label: "Raise",
    rule: 1,
    manual: false,
    review: false,
    ticked: true,
    overridden: null,
    held_back: false,
    delta_pct: 11.1,
    ...overrides,
  };
}

function page(overrides: Partial<ReviewPage> = {}): ReviewPage {
  return {
    source,
    data_version: 12,
    market: { loaded: true, version: 4, day: "2026-11-17", newest_price_date: "2026-11-16", covered: 3 },
    revision: 1,
    draft: false,
    rules: [
      {
        name: "Protect expensive cards",
        enabled: true,
        when: "Listed ≥ €50.00",
        then: "Keep",
        matched: 0,
        raised: 0,
        lowered: 0,
        value_delta: 0,
      },
      {
        name: "Follow the trend",
        enabled: true,
        when: "Every listing",
        then: "Trend",
        matched: 3,
        raised: 1,
        lowered: 1,
        value_delta: 0,
      },
    ],
    summary: {
      listings: 3,
      copies: 5,
      raises: 1,
      lowers: 1,
      review: 0,
      selected: 2,
      selected_copies: 3,
      value_before: 30,
      value_after: 30,
      held_back: 0,
      skipped: [],
    },
    total: 2,
    offset: 0,
    rows: [
      row("Riser"),
      row("Faller", {
        old_price: 12,
        new_price: 10,
        quantity: 1,
        outcome: "lower",
        outcome_label: "Lower",
        delta_pct: -16.7,
      }),
    ],
    dropped_ticks: 0,
    dropped_manual: 0,
    ...overrides,
  };
}

const market: MarketStatusView = {
  configured: true,
  job: { state: "idle" },
  loaded: { version: 4, loaded_at: "2026-11-17T08:00:00Z", day: "2026-11-17", cards: 3, products: 3 },
  stock_changed: false,
};

function server(user: MeResponse, routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: user },
    "GET /api/repricing/setup": { body: setup() },
    "GET /api/repricing/review": { body: page() },
    "GET /api/repricing/market/status": { body: market },
    "POST /api/repricing/market/load": { status: 202, body: market },
    "PUT /api/repricing/review/ticks": { body: { changed: 1 } },
    ...routes,
  });
}

const calls = (all: Call[], method: string, path: string) =>
  all.filter((c) => c.method === method && c.url.split("?")[0] === path);

afterEach(() => vi.unstubAllGlobals());

describe("repricing", () => {
  it("shows the rules, what they do and ticks a change", async () => {
    const { calls: all } = server(exporter);
    const { user } = renderApp("/repricing");
    const figures = await screen.findByRole("group", { name: "What the rules do" });
    expect(figures).toHaveTextContent("Changes2↑ 1 raises · ↓ 1 drops");
    expect(figures).toHaveTextContent("For export23 copies ticked");
    expect(screen.getByText("Published rules")).toBeInTheDocument();
    expect(screen.getByText(/revision 1 by Anna/)).toBeInTheDocument();
    const rules = screen.getByRole("region", { name: "Rules" });
    expect(within(rules).getByText("Follow the trend")).toBeInTheDocument();
    expect(within(rules).getByText("Every listing → Trend")).toBeInTheDocument();
    expect(within(rules).queryByRole("combobox", { name: "Add a rule" })).not.toBeInTheDocument();

    const table = screen.getByRole("table", { name: "Proposed prices" });
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    await user.click(within(table).getByRole("checkbox", { name: "Export the new price of Riser" }));
    await waitFor(() =>
      expect(calls(all, "PUT", "/api/repricing/review/ticks")[0]?.body).toEqual({
        version: 3,
        target: { piles: ["pile-Riser"] },
        ticked: false,
      }),
    );
    // The review asked for changes by impact, the published rules.
    const first = calls(all, "GET", "/api/repricing/review")[0]?.url ?? "";
    expect(first).toContain("draft=false");
    expect(first).toContain("filter=changes");
    expect(first).toContain("sort=impact");

    await user.click(screen.getByRole("button", { name: "Drops" }));
    await waitFor(() => expect(calls(all, "GET", "/api/repricing/review").at(-1)?.url).toContain("filter=lowers"));
    await user.click(within(rules).getByRole("button", { name: "3 listings" }));
    await waitFor(() => expect(calls(all, "GET", "/api/repricing/review").at(-1)?.url).toContain("rule=1"));
  });

  it("an editor's changes become a draft, then get published", async () => {
    const draft = { base_revision: 1, ruleset: { ...ruleset, name: "Changed" }, updated_at: "2026-11-17T09:00:00Z" };
    const { calls: all } = server(editor, {
      "PUT /api/repricing/draft": (call) => ({
        body: { draft: { ...draft, ruleset: (call.body as { ruleset: Ruleset }).ruleset } },
      }),
      "POST /api/repricing/publish": {
        body: { revision: 2, ruleset, updated_at: "2026-11-17T09:01:00Z", updated_by: "Anna" },
      },
    });
    const { user } = renderApp("/repricing");
    const rules = await screen.findByRole("region", { name: "Rules" });
    await user.click(within(rules).getByRole("button", { name: /Follow the trend/ }));
    const name = within(rules).getByLabelText("Name");
    await user.clear(name);
    await user.type(name, "Trend for all");
    expect(await screen.findByText("Unpublished changes")).toBeInTheDocument();
    await waitFor(() => expect(calls(all, "PUT", "/api/repricing/draft").length).toBeGreaterThan(0), { timeout: 3000 });
    const saved = calls(all, "PUT", "/api/repricing/draft").at(-1)?.body as { base_revision: number; ruleset: Ruleset };
    expect(saved.base_revision).toBe(1);
    expect(saved.ruleset.rules[1]?.name).toBe("Trend for all");

    // Price editor: a typed comma price goes in as a number.
    await user.type(within(rules).getByLabelText("At most"), "2,5");
    await user.click(screen.getByRole("button", { name: "Publish for everyone" }));
    await waitFor(() =>
      expect(calls(all, "POST", "/api/repricing/publish")[0]?.body).toEqual({ expected_revision: 1 }),
    );
    const flushed = calls(all, "PUT", "/api/repricing/draft").at(-1)?.body as { ruleset: Ruleset };
    expect(flushed.ruleset.rules[1]?.action.max_price).toBe(2.5);
  });

  it("a publish conflict lets the editor take the other rules", async () => {
    const draft = { base_revision: 1, ruleset: { ...ruleset, name: "Mine" }, updated_at: "2026-11-17T09:00:00Z" };
    const { calls: all } = server(editor, {
      "GET /api/repricing/setup": { body: setup({ draft }) },
      "PUT /api/repricing/draft": { body: { draft } },
      "POST /api/repricing/publish": {
        status: 409,
        body: { error: { code: "ruleset_conflict", message: "Ben published other rules meanwhile (revision 2)" } },
      },
      "DELETE /api/repricing/draft": { status: 204 },
    });
    const { user } = renderApp("/repricing");
    await user.click(await screen.findByRole("button", { name: "Publish for everyone" }));
    const dialog = await screen.findByRole("dialog", { name: "Someone published meanwhile" });
    expect(dialog).toHaveTextContent("Ben published other rules meanwhile (revision 2)");
    await user.click(within(dialog).getByRole("button", { name: "Discard mine, use theirs" }));
    await waitFor(() => expect(calls(all, "DELETE", "/api/repricing/draft")).toHaveLength(1));
    // With a draft, the page shows its prices; "Published" switches back.
    expect(calls(all, "GET", "/api/repricing/review")[0]?.url).toContain("draft=true");
  });

  it("viewers see the rules read-only and cannot tick or export", async () => {
    server(viewer);
    const { user } = renderApp("/repricing");
    const rules = await screen.findByRole("region", { name: "Rules" });
    await user.click(within(rules).getByRole("button", { name: /Follow the trend/ }));
    expect(within(rules).getByLabelText("Name")).toBeDisabled();
    expect(within(rules).getByRole("button", { name: "Raise and lower" })).toBeDisabled();
    expect(screen.queryByRole("tab", { name: "Exports" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Export/ })).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Export the new price of Riser" })).toBeDisabled();
  });

  it("the export warns about an old import and a big change, then saves both files", async () => {
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { calls: all } = server(exporter, {
      "POST /api/repricing/export": {
        body: {
          id: 7,
          file_name: "repricing_2026-11-17.csv",
          undo_file_name: "repricing_2026-11-17_undo.csv",
          changes: 2,
          raises: 1,
          lowers: 1,
        },
      },
      "GET /api/repricing/exports/7/csv": { body: "csv" },
      "GET /api/repricing/exports/7/undo": { body: "undo" },
    });
    const { user } = renderApp("/repricing");
    await user.click(await screen.findByRole("button", { name: "Export 2 changes…" }));
    const dialog = await screen.findByRole("dialog", { name: "Export new prices" });
    expect(await within(dialog).findByText(/That import is \d+ days old/)).toBeInTheDocument();
    expect(within(dialog).getByText(/This changes 67% of all 3 listings/)).toBeInTheDocument();
    expect(within(dialog).getByRole("table", { name: "Biggest changes" })).toHaveTextContent("Riser");
    await user.click(within(dialog).getByRole("button", { name: "Export and download both files" }));
    await waitFor(() => expect(names).toEqual(["repricing_2026-11-17.csv", "repricing_2026-11-17_undo.csv"]));
    expect(calls(all, "POST", "/api/repricing/export")[0]?.body).toEqual({
      revision: 1,
      data_version: 12,
      market_version: 4,
    });
    expect(within(dialog).getByText(/Saved/)).toBeInTheDocument();
  });

  it("explains one listing and sets a hand price", async () => {
    const detail: ListingExplained = {
      row: row("Riser"),
      steps: ["Rule 2: Follow the trend", "Trend €10.00", "New price €10.00"],
      market_date: "2026-11-16",
      market: [
        { label: "Trend", price: 10 },
        { label: "Low", price: null },
      ],
      age_days: 320,
      days_since_sale: 5,
      last_sale: "2026-11-12",
      sold_30d: 2,
      sales: [{ date: "2026-11-12", copies: 2 }],
      manual_price: null,
    };
    const { calls: all } = server(exporter, {
      "GET /api/repricing/listings/pile-Riser": { body: detail },
      "PUT /api/repricing/review/manual": { status: 204 },
    });
    const { user } = renderApp("/repricing");
    await user.click(await screen.findByRole("button", { name: "Riser" }));
    const dialog = await screen.findByRole("dialog", { name: "Riser" });
    expect(await within(dialog).findByText("Rule 2: Follow the trend")).toBeInTheDocument();
    expect(within(dialog).getByRole("table", { name: "Market prices" })).toHaveTextContent("Trend10.00 €");
    expect(dialog).toHaveTextContent("In stock for 320 days · 2 sold in 30 days · last sale 2026-11-12");
    await user.type(within(dialog).getByLabelText("Price by hand (€)"), "6,50");
    await user.click(within(dialog).getByRole("button", { name: "Use this price" }));
    await waitFor(() =>
      expect(calls(all, "PUT", "/api/repricing/review/manual")[0]?.body).toEqual({
        version: 3,
        pile: "pile-Riser",
        price: 6.5,
      }),
    );
  });

  it("asks for an import first", async () => {
    server(viewer, { "GET /api/repricing/setup": { body: setup({ source: null }) } });
    renderApp("/repricing");
    expect(await screen.findByText(/Repricing needs an inventory import/)).toBeInTheDocument();
  });
});
