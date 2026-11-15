import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DataInfo, LotRow, StockAnalysisView } from "../api/types";
import { admin, helper, reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { formatMargin, sortLots } from "./StockAnalysisPage";

function lot(overrides: Partial<LotRow> = {}): LotRow {
  return {
    lot: "L2",
    in_stock_listings: 1,
    in_stock_copies: 4,
    sold_copies: 6,
    bulk_copies: 0,
    in_stock_value: null,
    sold_revenue: null,
    bulk_revenue: null,
    cost: null,
    margin: null,
    recouped: null,
    cost_to_recoup: null,
    ...overrides,
  };
}

function view(money: boolean): StockAnalysisView {
  const m = (v: number) => (money ? v : null);
  return {
    money,
    total_articles: 4,
    in_stock_articles: 1,
    total_copies: 4,
    foil_count: 0,
    signed_count: 0,
    total_value: m(8),
    first_synced_date: "2026-01-01",
    top_by_quantity: [{ name: "Ancient", count: 4 }],
    top_by_price: [{ name: "Ancient", price: 2 }],
    longest_unsold: [{ name: "Ancient", date: "2026-01-01", price: 2, quantity: 4, location: "A-1-1-1-L2-R" }],
    oldest_listed: { name: "Ancient", date: "2026-01-01" },
    newest_listed: { name: "Ancient", date: "2026-01-01" },
    languages: [{ name: "English", count: 1 }],
    conditions: [{ name: "NM", count: 1 }],
    rarities: [
      { name: "common", count: 1 },
      { name: "", count: 2 },
    ],
    velocity: {
      period_days: 7,
      sold_copies: 6,
      copies_per_week: 6,
      last7_copies: 6,
      last30_copies: null,
      bulk_lots: 1,
      bulk_copies: 2,
      sold_revenue: m(13.5),
      revenue_per_week: m(13.5),
      bulk_revenue: m(1.5),
    },
    aging: [
      { label: "0–30 days", min_days: 0, max_days: 30, listings: 1, copies: 4, value: m(8) },
      { label: "31–90 days", min_days: 31, max_days: 90, listings: 0, copies: 0, value: m(0) },
    ],
    lots: [
      lot({
        bulk_copies: 2,
        in_stock_value: m(8),
        sold_revenue: m(13.5),
        bulk_revenue: m(1.5),
        cost: m(10),
        margin: m(0.35),
        recouped: money ? true : null,
        cost_to_recoup: m(0),
      }),
      lot({
        lot: "L10",
        in_stock_copies: 9,
        sold_copies: 1,
        in_stock_value: m(20),
        sold_revenue: m(2),
        cost: m(30),
        margin: m(-28 / 30),
        recouped: money ? false : null,
        cost_to_recoup: m(28),
      }),
    ],
    lots_sold_copies: 7,
    lot_totals: money ? { in_stock_value: 28, sold_revenue: 15.5, cost: 40, margin: -0.6125 } : null,
    bulk_lots: 1,
    bulk_copies: 2,
    bulk_revenue: m(1.5),
  };
}

const dataInfo: DataInfo = {
  copy_taken_at: "2026-01-16T08:00:00Z",
  last_import: "2026-01-15",
  inventory_version: 1,
  updated_at: "2026-01-16T08:00:00Z",
  test_copy: true,
};

describe("stock analysis", () => {
  it("shows counts and sales speed but no money to analysts", async () => {
    fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/stock-analysis": { body: view(false) },
    });
    renderApp("/stock-analysis");
    expect(await screen.findByRole("heading", { name: "Stock analysis" })).toBeInTheDocument();
    expect(await screen.findByText("Copies / week")).toBeInTheDocument();
    for (const money of ["Stock value", "Revenue", "Revenue / week"]) {
      expect(screen.queryByText(money, { selector: ".kpi__label" }), money).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("columnheader", { name: "Capital" })).not.toBeInTheDocument();
    for (const money of ["Stock value", "Revenue", "Cost", "Margin", "Payback"]) {
      expect(screen.queryByRole("columnheader", { name: money }), money).not.toBeInTheDocument();
    }
    const l2 = screen.getByRole("row", { name: /^L2/ });
    expect(l2).toHaveTextContent("×4");
    expect(l2).toHaveTextContent("×6");
    expect(l2).not.toHaveTextContent("€");
    expect(screen.getByText(/need the permission “View revenue and costs”/)).toBeInTheDocument();
    expect(screen.getByText(/Includes 2 cards sold in 1 bulk lot\./)).toBeInTheDocument();
    // Card prices are listing prices, not money totals.
    expect(screen.getByRole("row", { name: /Ancient/ })).toHaveTextContent("2.00 €");
    expect(screen.getByText(/Data as of 2026-01-15/)).toBeInTheDocument();
    expect(screen.getByText("Rarities:").parentElement).toHaveTextContent("common 1 · unknown 2");
  });

  it("shows value, revenue, cost, margin and payback to the owner", async () => {
    fakeServer({
      "GET /api/auth/me": { body: admin },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/stock-analysis": { body: view(true) },
    });
    renderApp("/stock-analysis");
    const value = await screen.findByText("Stock value", { selector: ".kpi__label" });
    expect(value.closest(".kpi")).toHaveTextContent("8.00 €");
    expect(screen.getByText("Revenue", { selector: ".kpi__label" }).closest(".kpi")).toHaveTextContent("13.50 €");
    expect(screen.getByRole("columnheader", { name: "Capital" })).toBeInTheDocument();
    const l2 = screen.getByRole("row", { name: /^L2/ });
    expect(l2).toHaveTextContent("13.50 €");
    expect(l2).toHaveTextContent("+35%");
    expect(l2).toHaveTextContent("✓ recouped");
    const l10 = screen.getByRole("row", { name: /^L10/ });
    expect(l10).toHaveTextContent("−93%");
    expect(l10).toHaveTextContent("28.00 € to go");
    const total = screen.getByRole("row", { name: /^Total.*×7/ });
    expect(total).toHaveTextContent("40.00 €");
    expect(total).toHaveTextContent("−61%");
    expect(screen.getByText(/for 1\.50 € \(each card earns/)).toBeInTheDocument();
  });

  it("sorts the lot table on the page", async () => {
    fakeServer({
      "GET /api/auth/me": { body: admin },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/stock-analysis": { body: view(true) },
    });
    const { user } = renderApp("/stock-analysis");
    const table = await screen.findByRole("table", { name: "Purchase lots" });
    const header = (name: RegExp) => within(table).getByRole("columnheader", { name });
    const lots = () =>
      within(table)
        .getAllByRole("row")
        .map((r) => within(r).queryByRole("rowheader")?.textContent)
        .filter((t) => t?.startsWith("L"));
    await screen.findByRole("row", { name: /^L10/ });
    expect(lots()).toEqual(["L2", "L10"]);
    await user.click(within(header(/Copies/)).getByRole("button"));
    expect(lots()).toEqual(["L10", "L2"]);
    expect(header(/Copies/)).toHaveAttribute("aria-sort", "descending");
    await user.click(within(header(/Copies/)).getByRole("button"));
    expect(lots()).toEqual(["L2", "L10"]);
    await user.click(within(header(/Margin/)).getByRole("button"));
    expect(lots()).toEqual(["L2", "L10"]);
    await user.click(within(header(/Margin/)).getByRole("button"));
    expect(lots()).toEqual(["L10", "L2"]);
  });

  it("explains a missing sales history", async () => {
    fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/stock-analysis": { body: { ...view(false), velocity: null, lots: [] } },
    });
    renderApp("/stock-analysis");
    expect(await screen.findByText(/Not enough history yet/)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Purchase lots" })).not.toBeInTheDocument();
  });

  it("is refused without the report permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/stock-analysis");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
  });

  it("sorts lots by number and puts missing values last", () => {
    const rows = [lot({ lot: "L10", cost: 5 }), lot({ lot: "L2", cost: null }), lot({ lot: "L3", cost: 1 })];
    expect(sortLots(rows, "lot", false).map((r) => r.lot)).toEqual(["L2", "L3", "L10"]);
    expect(sortLots(rows, "lot", true).map((r) => r.lot)).toEqual(["L10", "L3", "L2"]);
    expect(sortLots(rows, "cost", true).map((r) => r.lot)).toEqual(["L10", "L3", "L2"]);
    expect(sortLots(rows, "cost", false).map((r) => r.lot)).toEqual(["L3", "L10", "L2"]);
    expect(formatMargin(0.35)).toBe("+35%");
    expect(formatMargin(-0.5)).toBe("−50%");
    expect(formatMargin(null)).toBe("—");
  });
});
