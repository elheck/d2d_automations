import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MeResponse, PileRow, SearchView } from "../api/types";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";
import { addToSelection, formatPct, selectionTotals } from "./searchPage";

const searcher: MeResponse = { username: "searcher", display_name: "Searcher", permissions: ["search.use"] };
const clerk: MeResponse = {
  username: "clerk",
  display_name: "Clerk",
  permissions: ["search.use", "inventory.discard"],
};

function pile(overrides: Partial<PileRow> = {}): PileRow {
  return {
    id: "aaaaaaaaaaaaaaaa",
    name: "Spark",
    set_name: "Test Set",
    set_code: "TST",
    collector_number: "2",
    condition: "NM",
    language: "English",
    foil: false,
    signed: false,
    quantity: 3,
    price: 1.5,
    location: "A-0-0-2-L2-R",
    rarity: "Common",
    comment: "",
    product_id: 2,
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=2",
    ...overrides,
  };
}

const source = { version: 3, file_name: "inventory-report-Magic.csv", imported_at: "2026-11-15T10:00:00Z" };

function view(rows: PileRow[] = [pile(), pile({ id: "bbbbbbbbbbbbbbbb", name: "Ember", quantity: 1 })]): SearchView {
  return {
    source,
    total: rows.length,
    copies: rows.reduce((n, r) => n + r.quantity, 0),
    page: 0,
    per_page: 100,
    rows,
  };
}

function server(user: MeResponse, routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: user },
    "GET /api/search": { body: view() },
    "GET /api/inventory/discards": { body: [] },
    ...routes,
  });
}

const searches = (calls: Call[]) => calls.filter((c) => c.url.startsWith("/api/search?")).map((c) => c.url);

afterEach(() => vi.unstubAllGlobals());

describe("search", () => {
  it("searches as you type in the chosen fields", async () => {
    const { calls } = server(searcher);
    const { user } = renderApp("/search");
    expect(await screen.findByRole("row", { name: /Spark/ })).toHaveTextContent("A-0-0-2-L2-R");
    expect(screen.getByText(/2 piles · 4 copies/)).toBeInTheDocument();
    expect(searches(calls)[0]).toBe(
      "/api/search?q=&fields=name%2Cset&case_sensitive=false&all_languages=false&page=0&per_page=100",
    );
    await user.type(screen.getByRole("searchbox", { name: "Search" }), "spa");
    await user.click(screen.getByLabelText("Location"));
    await waitFor(() => expect(searches(calls).at(-1)).toContain("q=spa&fields=name%2Cset%2Clocation"));
    // Searchers without the discard permission can select but not discard.
    await user.click(screen.getByRole("button", { name: "Select one Spark" }));
    expect(screen.getByRole("region", { name: "Selection" })).toHaveTextContent("Selected: 1 copy in 1 pile");
    expect(screen.queryByRole("button", { name: "Discard…" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Recent discards" })).not.toBeInTheDocument();
  });

  it("discards after the order warning and downloads the CSV", async () => {
    const { calls } = server(clerk, {
      "POST /api/inventory/discard": { body: { id: 7, copies: 3, variants: 1, value: null, data_version: 9 } },
      "GET /api/inventory/discards/7/csv": { body: "csv" },
    });
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = renderApp("/search");
    await user.click(await screen.findByRole("button", { name: "Select all Spark" }));
    expect(screen.getByRole("button", { name: "Select one Spark" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Discard…" }));
    const dialog = screen.getByRole("dialog", { name: "Import order matters for discards" });
    expect(dialog).toHaveTextContent("You are writing off 3 copies.");
    expect(dialog).toHaveTextContent("Import it into TCG Powertools right away");
    expect(calls.some((c) => c.method === "POST")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Discard and download the CSV" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Discarded 3 copies.");
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({
      items: [{ pile: "aaaaaaaaaaaaaaaa", quantity: 3 }],
    });
    await waitFor(() => expect(names).toEqual(["discarded_cards_7.csv"]));
    expect(screen.queryByRole("region", { name: "Selection" })).not.toBeInTheDocument();
    // The user's own discard raises no "inventory updated" banner.
    expect(screen.queryByText(/The inventory was updated/)).not.toBeInTheDocument();
  });

  it("shows a refusal in the dialog and keeps the selection", async () => {
    server(clerk, {
      "POST /api/inventory/discard": {
        status: 409,
        body: { error: { code: "not_enough", message: 'Only 1 copy of "Spark" is left' } },
      },
    });
    const { user } = renderApp("/search");
    await user.click(await screen.findByRole("button", { name: "Select all Spark" }));
    await user.click(screen.getByRole("button", { name: "Discard…" }));
    await user.click(screen.getByRole("button", { name: "Discard and download the CSV" }));
    expect(await screen.findByText('Only 1 copy of "Spark" is left')).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Selection" })).toBeInTheDocument();
  });

  it("lists recent discards with their CSV", async () => {
    server(clerk, {
      "GET /api/inventory/discards": {
        body: [
          {
            id: 7,
            created_at: "2026-11-15T10:00:00Z",
            user_name: "Clerk",
            copies: 3,
            variants: 1,
            settled: false,
            csv_available: true,
          },
          {
            id: 2,
            created_at: "2026-11-01T10:00:00Z",
            user_name: "Clerk",
            copies: 1,
            variants: 1,
            settled: true,
            csv_available: false,
          },
        ],
      },
    });
    renderApp("/search");
    const table = await screen.findByRole("table", { name: "Recent discards" });
    expect(within(table).getAllByRole("row")[1]).toHaveTextContent("Waiting for the next import");
    expect(within(table).getAllByRole("row")[2]).toHaveTextContent("no longer kept");
  });

  it("shows the price history of a card", async () => {
    server(searcher, {
      "GET /api/search/history/2": {
        body: {
          name: "Spark",
          expansion: "Test Set",
          foil: false,
          points: [
            { date: "2026-11-01", price: 1 },
            { date: "2026-11-15", price: 1.5 },
          ],
          trend: 1.5,
          change_7d: 50,
          change_30d: null,
        },
      },
    });
    const { user } = renderApp("/search");
    await user.click(await screen.findByRole("button", { name: "Price history of Spark" }));
    const dialog = await screen.findByRole("dialog", { name: "Price history — Spark" });
    expect(await within(dialog).findByText(/7 days \+50\.0%/)).toHaveTextContent(
      "Trend 1.50 € · 7 days +50.0% · 30 days —",
    );
  });

  it("asks for an import first", async () => {
    server(searcher, { "GET /api/search": { body: { ...view([]), source: null } } });
    renderApp("/search");
    expect(await screen.findByText(/Search needs an inventory import on the server/)).toBeInTheDocument();
  });

  it("selection rules", () => {
    let s = addToSelection([], pile(), 2);
    s = addToSelection(s, pile(), 5);
    expect(s).toHaveLength(1);
    expect(s[0]?.quantity).toBe(3);
    expect(selectionTotals(s)).toEqual({ copies: 3, value: 4.5 });
    expect(formatPct(-4)).toBe("−4.0%");
    expect(formatPct(null)).toBe("—");
  });
});
