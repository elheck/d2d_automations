import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { BulkLotView, BulkOptions, BulkPreview, DraftView, LotLine, MeResponse, SpecView } from "../api/types";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";
import { daysText, parsePrice, pickedTotals, takenLines } from "./bulkLots";

const seller: MeResponse = {
  username: "seller",
  display_name: "Seller",
  permissions: ["bulk_lot.build", "bulk_lot.finish", "bulk_lot.manage"],
};
const builder: MeResponse = { username: "builder", display_name: "Builder", permissions: ["bulk_lot.build"] };

const spec: SpecView = {
  size: 4,
  rarities: ["common"],
  card_type: "any",
  exclude_basic_lands: true,
  foil: "non_foil",
  excluded_conditions: [],
  excluded_languages: [],
  max_price: 0.25,
  min_age_days: 90,
  max_copies_per_name: 1,
  hits: 0,
  hit_min_price: 0.5,
  order: "nearby",
  seed: 1,
};

const source = { version: 3, file_name: "inventory-report-Magic.csv", imported_at: "2026-11-15T10:00:00Z" };

const options: BulkOptions = {
  source,
  default_spec: spec,
  presets: [{ label: "100 commons", spec: { ...spec, size: 100 } }],
  conditions: ["NM", "EX", "GD", "LP", "PL", "PO"],
  languages: [
    { language: "English", copies: 30 },
    { language: "German", copies: 10 },
  ],
  max_lot_price: 100000,
  max_name_chars: 120,
};

function line(i: number, overrides: Partial<LotLine> = {}): LotLine {
  return {
    pile: `pile${i}`,
    copies: 1,
    hit: false,
    picked: false,
    free: 2,
    name: `Common ${i}`,
    set_code: "TST",
    collector_number: String(i),
    condition: "NM",
    language: "English",
    foil: false,
    price: 0.1,
    location: `A-0-0-${i}-L3-R`,
    purchase_lot: "L3",
    age_days: 300,
    image: `/api/bulk-lots/image/tst/${i}`,
    ...overrides,
  };
}

const summary = {
  copies: 4,
  listed_value: 0.4,
  hits: 0,
  distinct_names: 4,
  oldest_days: 300,
  newest_days: 300,
  purchase_lots: [["L3", 4]] as [string, number][],
};

function preview(overrides: Partial<BulkPreview> = {}): BulkPreview {
  return {
    source,
    size: 4,
    funnel: {
      steps: [
        { label: "In stock", copies: 13 },
        { label: "Rarity", copies: 12 },
      ],
      reserved: 0,
      types_unknown: 0,
      hit_pool: 0,
      bottleneck: null,
    },
    lines: [1, 2, 3, 4].map((i) => line(i)),
    summary,
    default_name: "4 Commons",
    types_needed: 0,
    types: { state: "idle" },
    ...overrides,
  };
}

function draft(overrides: Partial<DraftView> = {}): DraftView {
  return {
    spec,
    default_name: "4 Commons",
    lines: [1, 2, 3, 4].map((i) => line(i)),
    gone: 0,
    summary,
    updated_at: "2026-11-16T10:00:00Z",
    ...overrides,
  };
}

const lot: BulkLotView = {
  id: 7,
  name: "4 Commons",
  created_at: "2026-11-16",
  copies: 4,
  pending_copies: 4,
  status: "awaiting_import",
  price: 2,
  listed_value: 0.4,
  csv_available: true,
};

function server(user: MeResponse, routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: user },
    "GET /api/bulk-lots/options": { body: options },
    "POST /api/bulk-lots/preview": { body: preview() },
    "GET /api/bulk-lots/types/status": { body: { state: "idle" } },
    "GET /api/bulk-lots/draft": { body: null },
    "POST /api/bulk-lots/draft": { body: draft() },
    "GET /api/bulk-lots": { body: [lot] },
    ...routes,
  });
}

const posts = (calls: Call[], path: string) => calls.filter((c) => c.method === "POST" && c.url === path);

afterEach(() => vi.unstubAllGlobals());

describe("bulk lots", () => {
  it("previews the rules and starts picking", async () => {
    const { calls } = server(seller);
    const { user } = renderApp("/bulk-lots");
    const stats = await screen.findByRole("group", { name: "The lot" });
    await waitFor(() => expect(stats).toHaveTextContent("Cards4 / 44 different names"));
    expect(stats).toHaveTextContent("Single value0.40 €listed price of these cards");
    expect(stats).toHaveTextContent("In stock for300 daysall the same age");
    expect(within(stats).queryByText("Hits")).not.toBeInTheDocument();
    expect(screen.getByRole("table", { name: "Cards of the lot" })).toHaveTextContent("A-0-0-1-L3-R");
    expect(screen.getByText("From purchase lots L3 ×4")).toBeInTheDocument();
    const funnel = screen.getByRole("region", { name: "How the cards were chosen" });
    expect(
      within(funnel)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["In stock13", "Rarity12"]);
    expect(within(funnel).queryByText("tightest rule")).not.toBeInTheDocument();

    // Chips: pressed = may go into the lot.
    const german = screen.getByRole("button", { name: /^German 10/ });
    expect(german).toHaveAttribute("aria-pressed", "true");
    await user.click(german);
    expect(german).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "PO" }));
    await user.click(screen.getByRole("button", { name: "Uncommon" }));
    await user.click(screen.getByRole("button", { name: "Both" }));
    await waitFor(() =>
      expect(posts(calls, "/api/bulk-lots/preview").at(-1)?.body).toMatchObject({
        spec: {
          excluded_languages: ["German"],
          excluded_conditions: ["PO"],
          rarities: ["common", "uncommon"],
          foil: "any",
        },
      }),
    );
    expect(screen.getByRole("button", { name: "Both" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Non-foil" })).toHaveAttribute("aria-pressed", "false");

    await user.click(screen.getByRole("button", { name: "Start picking" }));
    expect(await screen.findByText("Picked 0 of 4")).toBeInTheDocument();
    expect(posts(calls, "/api/bulk-lots/draft")[0]?.body).toMatchObject({ spec: { excluded_languages: ["German"] } });
  });

  it("a preset keeps the condition, language, finish and age choices", async () => {
    const { calls } = server(seller);
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("button", { name: /^German 10/ }));
    await user.clear(screen.getByLabelText("In stock at least"));
    await user.type(screen.getByLabelText("In stock at least"), "30");
    await user.click(screen.getByRole("button", { name: "100 commons" }));
    expect(screen.getByLabelText("Cards in the lot")).toHaveValue(100);
    await waitFor(() =>
      expect(posts(calls, "/api/bulk-lots/preview").at(-1)?.body).toMatchObject({
        spec: { size: 100, excluded_languages: ["German"], min_age_days: 30 },
      }),
    );
  });

  it("shows hits and a short lot with its tightest rule", async () => {
    server(seller, {
      "POST /api/bulk-lots/preview": {
        body: preview({
          summary: { ...summary, copies: 3, hits: 1 },
          funnel: { ...preview().funnel, bottleneck: "Rarity" },
        }),
      },
    });
    const { user } = renderApp("/bulk-lots");
    await user.clear(await screen.findByLabelText("Guaranteed hits"));
    await user.type(screen.getByLabelText("Guaranteed hits"), "2");
    const stats = screen.getByRole("group", { name: "The lot" });
    await waitFor(() => expect(stats).toHaveTextContent("Hits1worth ≥ 0.50 €"));
    expect(stats).toHaveTextContent("Cards3 / 4");
    expect(screen.getByText(/The tightest rule is “Rarity”/)).toBeInTheDocument();
    expect(screen.getByText("Only 3 of 4 match.")).toBeInTheDocument();
    const funnel = screen.getByRole("region", { name: "How the cards were chosen" });
    const tight = within(funnel).getByText("tightest rule").closest("li");
    expect(tight).toHaveTextContent("Rarity");
    expect(tight).toHaveClass("funnel__step--tight");
  });

  it("asks for card types and shows the limiting rule", async () => {
    const { calls } = server(seller, {
      "POST /api/bulk-lots/preview": {
        body: preview({
          types_needed: 6,
          summary: { ...summary, copies: 0 },
          lines: [],
          funnel: { ...preview().funnel, bottleneck: "Card type" },
        }),
      },
      "POST /api/bulk-lots/types/load": {
        status: 202,
        body: { state: "running", started_at: "x", stage: "Card types", done: 1, total: 2 },
      },
    });
    const { user } = renderApp("/bulk-lots");
    expect(await screen.findByText(/6 cards need their card type from Scryfall/)).toBeInTheDocument();
    expect(screen.getByText(/The tightest rule is “Card type”/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start picking" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Look up card types" }));
    await waitFor(() => expect(posts(calls, "/api/bulk-lots/types/load")).toHaveLength(1));
  });

  it("picks, replaces and finishes with the CSV", async () => {
    const lines = [1, 2, 3, 4].map((i) => line(i, { picked: i !== 2 }));
    const { calls } = server(seller, {
      "GET /api/bulk-lots/draft": { body: draft({ lines }) },
      "POST /api/bulk-lots/draft/replace": { body: { draft: draft({ lines }), added: 1 } },
      "POST /api/bulk-lots/finish": { body: { id: 7, name: "4 Commons", copies: 3, price: 2, data_version: 12 } },
      "GET /api/bulk-lots/7/csv": { body: "csv" },
    });
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("tab", { name: "2. Pick (4)" }));
    expect(screen.getByText("Picked 3 of 4")).toBeInTheDocument();
    const second = screen.getByRole("article", { name: "Common 2" });
    await user.click(within(second).getByRole("button", { name: "Replace" }));
    await waitFor(() => expect(posts(calls, "/api/bulk-lots/draft/replace")[0]?.body).toEqual({ pile: "pile2" }));

    await user.click(screen.getByRole("button", { name: "Finish the lot…" }));
    const dialog = screen.getByRole("dialog", { name: "Finish the lot" });
    expect(within(dialog).getByLabelText("Name")).toHaveValue("4 Commons");
    const record = within(dialog).getByRole("button", { name: "Record the lot and download the CSV" });
    expect(record).toBeDisabled();
    await user.type(within(dialog).getByLabelText("Price the lot sells for (€)"), "1,50");
    expect(dialog).toHaveTextContent("3 picked copies in 3 lines, listed at 0.30 € · 0.50 € per card.");
    await user.click(record);
    await waitFor(() => expect(names).toEqual(["bulk_lot_7.csv"]));
    expect(posts(calls, "/api/bulk-lots/finish")[0]?.body).toEqual({ name: "4 Commons", price: 1.5 });
  });

  it("warns about cards another lot took meanwhile", async () => {
    server(seller, {
      "GET /api/bulk-lots/draft": { body: draft({ lines: [line(1, { free: 0 }), line(2)], gone: 1 }) },
    });
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("tab", { name: "2. Pick (2)" }));
    expect(screen.getByRole("alert")).toHaveTextContent("1 card was taken by another lot or a discard meanwhile");
    expect(screen.getByText(/1 card is no longer in the stock/)).toBeInTheDocument();
    expect(
      within(screen.getByRole("article", { name: "Common 1" })).getByText("Taken meanwhile (0 free)"),
    ).toBeInTheDocument();
  });

  it("builders choose and pick but neither finish nor see prices", async () => {
    server(builder, {
      "GET /api/bulk-lots/draft": { body: draft() },
      "GET /api/bulk-lots": { body: [{ ...lot, price: null, listed_value: null }] },
    });
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("tab", { name: "2. Pick (4)" }));
    expect(screen.queryByRole("button", { name: "Finish the lot…" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "History" }));
    const table = await screen.findByRole("table", { name: "Bulk lots" });
    expect(within(table).queryByRole("columnheader", { name: "Price" })).not.toBeInTheDocument();
    expect(within(table).queryByRole("button", { name: "Undo…" })).not.toBeInTheDocument();
    expect(within(table).queryByRole("button", { name: "CSV" })).not.toBeInTheDocument();
  });

  it("undoing a lot an import already took copies of needs their number", async () => {
    const { calls } = server(seller, {
      "GET /api/bulk-lots": { body: [{ ...lot, pending_copies: 1, status: "overdue" }] },
      "POST /api/bulk-lots/7/undo": { body: { data_version: 20 } },
    });
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("tab", { name: "History" }));
    expect(await screen.findByText("Awaiting import for over 7 days")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Undo…" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("An import already took 3 copies of this lot out.");
    const undo = within(dialog).getByRole("button", { name: "Undo the lot" });
    expect(undo).toBeDisabled();
    await user.type(within(dialog).getByLabelText("Copies already taken out"), "3");
    await user.click(undo);
    await waitFor(() => expect(posts(calls, "/api/bulk-lots/7/undo")[0]?.body).toEqual({ confirm_removed: "3" }));
  });

  it("corrects a price with the one it showed", async () => {
    const { calls } = server(seller, { "PUT /api/bulk-lots/7/price": { body: { data_version: 21 } } });
    const { user } = renderApp("/bulk-lots");
    await user.click(await screen.findByRole("tab", { name: "History" }));
    await user.click(await screen.findByRole("button", { name: "Price…" }));
    const input = screen.getByLabelText("Price it sold for (€)");
    await user.clear(input);
    await user.type(input, "3");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ price: 3, expected: 2 }));
  });

  it("rules", () => {
    expect(parsePrice("1,50")).toBe(1.5);
    expect(parsePrice("100000")).toBe(100000);
    for (const bad of ["", "-1", "100000.01", "1.234", "x"]) expect(parsePrice(bad), bad).toBeNull();
    const lines = [line(1, { picked: true, copies: 2 }), line(2), line(3, { free: 0 })];
    expect(pickedTotals(lines)).toEqual({ lines: 1, copies: 2, listedValue: 0.2 });
    expect(takenLines(lines).map((l) => l.pile)).toEqual(["pile3"]);
    expect(daysText(1)).toBe("1 day");
    expect(daysText(null)).toBe("—");
  });
});
