import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RestockRow, RestockView } from "../api/types";
import { admin, helper, reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call } from "../test/server";
import { nextRestockSort, parseMinCopies } from "./RestockPage";

function row(overrides: Partial<RestockRow> = {}): RestockRow {
  return {
    name: "Bolt",
    set_code: "TST",
    cn: "1",
    condition: "NM",
    language: "English",
    is_foil: false,
    rarity: "common",
    lot: "L2",
    sold_copies: 3,
    copies_per_week: 3,
    days_to_sell_out: 7,
    last_price: 1,
    realized_revenue: null,
    sold_out_date: "2026-01-08",
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=101",
    ...overrides,
  };
}

const view = (money: boolean): RestockView => ({
  rows: [
    row({ realized_revenue: money ? 3 : null }),
    row({
      name: "Shock",
      is_foil: true,
      sold_copies: 2,
      copies_per_week: 1,
      days_to_sell_out: 14,
      last_price: 5,
      realized_revenue: money ? 10 : null,
      cardmarket_url: null,
    }),
  ],
  total: 2,
  copies: 5,
  revenue: money ? 13 : null,
});

const dataInfo = { copy_taken_at: "2026-01-16T08:00:00Z", last_import: "2026-01-15" };
const restockCalls = (calls: Call[]) => calls.filter((c) => c.url.startsWith("/api/restock?"));

afterEach(() => vi.unstubAllGlobals());

describe("restock", () => {
  it("lists sold-out cards without money for analysts", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/restock": { body: view(false) },
    });
    renderApp("/restock");
    const bolt = await screen.findByRole("row", { name: /Bolt/ });
    expect(bolt).toHaveTextContent("×3");
    expect(bolt).toHaveTextContent("3.00");
    expect(bolt).toHaveTextContent("1.00 €");
    expect(bolt).toHaveTextContent("L2");
    expect(within(bolt).getByRole("link", { name: "Bolt" })).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByRole("row", { name: /Shock/ })).toHaveTextContent("✦");
    expect(screen.queryByRole("columnheader", { name: /Revenue/ })).not.toBeInTheDocument();
    expect(screen.queryByText("Realized revenue")).not.toBeInTheDocument();
    expect(screen.getByText(/Data as of 2026-01-15 \(last inventory import\)/)).toBeInTheDocument();
    expect(restockCalls(calls)[0]?.url).toBe("/api/restock?min_copies=2&sort=velocity&desc=true");
  });

  it("shows revenue to users who may see money", async () => {
    fakeServer({
      "GET /api/auth/me": { body: admin },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/restock": { body: view(true) },
    });
    renderApp("/restock");
    expect(await screen.findByRole("columnheader", { name: /Revenue/ })).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Shock/ })).toHaveTextContent("10.00 €");
    expect(screen.getByText("Realized revenue").closest(".kpi")).toHaveTextContent("13.00 €");
  });

  it("sorts and filters on the server", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/restock": { body: view(false) },
    });
    const { user } = renderApp("/restock");
    await screen.findByRole("row", { name: /Bolt/ });
    await user.click(within(screen.getByRole("columnheader", { name: /Card/ })).getByRole("button"));
    await waitFor(() => expect(restockCalls(calls).at(-1)?.url).toContain("sort=name&desc=false"));
    const min = screen.getByLabelText("Min copies sold");
    await user.clear(min);
    await user.type(min, "5");
    await waitFor(() => expect(restockCalls(calls).at(-1)?.url).toBe("/api/restock?min_copies=5&sort=name&desc=false"));
    await user.clear(min);
    await user.type(min, "0");
    expect(screen.getByText("Enter a whole number from 1 to 1000.")).toBeInTheDocument();
  });

  it("downloads the buy list", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": { body: dataInfo },
      "GET /api/restock": { body: view(false) },
      "GET /api/restock/export": { body: "csv" },
    });
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = renderApp("/restock");
    await user.click(await screen.findByRole("button", { name: "Download buy list" }));
    await waitFor(() => expect(names).toEqual(["restock_buy_list.csv"]));
    expect(calls.find((c) => c.url.startsWith("/api/restock/export"))?.method).toBe("GET");
  });

  it("is in the reports menu only for users who may see reports", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/restock");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).not.toHaveTextContent("Reports");
  });

  it("accepts whole numbers of copies and sorts like the desktop app", () => {
    expect(parseMinCopies("2")).toBe(2);
    expect(parseMinCopies(" 1000 ")).toBe(1000);
    for (const bad of ["", "0", "1001", "1.5", "-1", "abc"]) expect(parseMinCopies(bad), bad).toBeNull();
    expect(nextRestockSort({ sort: "velocity", desc: true }, "velocity")).toEqual({ sort: "velocity", desc: false });
    expect(nextRestockSort({ sort: "velocity", desc: true }, "price")).toEqual({ sort: "price", desc: true });
    expect(nextRestockSort({ sort: "velocity", desc: true }, "name")).toEqual({ sort: "name", desc: false });
  });
});
