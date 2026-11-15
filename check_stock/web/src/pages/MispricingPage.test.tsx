import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MarketStatusView, MispricingRow, MispricingView } from "../api/types";
import { admin, pricer, reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call } from "../test/server";
import { actionClass, marketText, nextMispricingSort, verdictText } from "./MispricingPage";

const loaded: MarketStatusView = {
  configured: true,
  job: { state: "finished", at: "2026-11-15T08:00:00Z" },
  loaded: { version: 1, loaded_at: "2026-11-15T08:00:00Z", day: "2026-11-15", cards: 3, products: 3 },
  stock_changed: false,
};

function row(overrides: Partial<MispricingRow> = {}): MispricingRow {
  return {
    name: "Riser",
    set_code: "TST",
    condition: "NM",
    language: "English",
    is_foil: false,
    location: "A-1-1-1-L2-R",
    quantity: 2,
    age_days: 318,
    listed: 9,
    market: 10,
    condition_factor: 1,
    market_low: 8,
    delta_pct: -10,
    impact: 2,
    verdict: "underpriced",
    below_low: false,
    recently_listed: false,
    language_flagged: false,
    stale: false,
    effective_threshold_pct: 5,
    momentum: "rising",
    action: "raise_now",
    priority: 4,
    pct_7d: 1,
    pct_30d: 3,
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=101",
    ...overrides,
  };
}

function view(money: boolean): MispricingView {
  return {
    version: 1,
    summary: {
      underpriced_rows: 1,
      underpriced_copies: 2,
      overpriced_rows: 1,
      overpriced_copies: 1,
      fair_rows: 1,
      no_data_rows: 0,
      raise_now_rows: 1,
      cut_now_rows: 0,
      stale_rows: 1,
      upside: money ? 2 : null,
      excess: money ? 2 : null,
      listed_value: money ? 50 : null,
      market_value: money ? 50 : null,
    },
    rows: [
      row(),
      row({
        name: "Faller",
        condition: "EX",
        condition_factor: 0.9,
        market: 9,
        listed: 12,
        delta_pct: 33.3,
        impact: 3,
        verdict: "overpriced",
        stale: true,
        action: "cut",
        language: "Italian",
        language_flagged: true,
        cardmarket_url: null,
      }),
    ],
    total: 2,
    conflicts: [
      { kind: "condition_inversion", name: "Flat", set_code: "TST", details: "NM €5.00 @ A-1 vs EX €7.00 @ A-2" },
    ],
    conflicts_total: 1,
  };
}

const reportCalls = (calls: Call[]) => calls.filter((c) => c.url.startsWith("/api/mispricing?"));

function server(me = pricer, money = false, extra: Parameters<typeof fakeServer>[0] = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: me },
    "GET /api/data-info": {
      body: {
        copy_taken_at: null,
        last_import: "2026-11-11",
        inventory_version: 1,
        updated_at: null,
        test_copy: false,
      },
    },
    "GET /api/mispricing/status": { body: loaded },
    "POST /api/mispricing/load": { status: 202, body: loaded },
    "GET /api/mispricing": { body: view(money) },
    ...extra,
  });
}

describe("mispricing", () => {
  it("judges listings against the market, without money totals for pricers", async () => {
    const { calls } = server();
    const { user } = renderApp("/mispricing");
    const riser = await screen.findByRole("row", { name: /Riser/ });
    expect(riser).toHaveTextContent("−10%");
    expect(riser).toHaveTextContent("Underpriced");
    expect(within(riser).getByText("Raise now")).toHaveClass("action--now");
    const faller = screen.getByRole("row", { name: /Faller/ });
    expect(faller).toHaveTextContent("9.00 €* ⚠");
    expect(faller).toHaveTextContent("Overpriced · lang");
    expect(within(faller).getByText("Overpriced · lang")).toHaveAttribute(
      "title",
      expect.stringContaining("Italian copy"),
    );

    expect(screen.getByText("Underpriced", { selector: ".kpi__label" }).closest(".kpi")).toHaveTextContent("1 · ×2");
    expect(screen.queryByText("Upside")).not.toBeInTheDocument();
    expect(screen.queryByText("Listed value*")).not.toBeInTheDocument();

    await user.click(screen.getByText("Internal consistency: 1 conflicts"));
    expect(screen.getByRole("row", { name: /Condition inversion/ })).toHaveTextContent(
      "NM €5.00 @ A-1 vs EX €7.00 @ A-2",
    );

    expect(calls.filter((c) => c.url === "/api/mispricing/load").map((c) => c.body)).toEqual([{ refresh: false }]);
    expect(reportCalls(calls)[0]?.url).toBe(
      "/api/mispricing?field=trend&threshold=15&filter=all&sort=priority&desc=true",
    );
  });

  it("shows money totals to users who may see them", async () => {
    server(admin, true);
    renderApp("/mispricing");
    expect(await screen.findByText("Upside")).toBeInTheDocument();
    expect(screen.getByText("Upside").closest(".kpi")).toHaveTextContent("2.00 €");
    expect(screen.getByText("Listed value*").closest(".kpi")).toHaveTextContent("50.00 €");
  });

  it("filters, sorts and checks the band", async () => {
    const { calls } = server();
    const { user } = renderApp("/mispricing");
    await screen.findByRole("row", { name: /Riser/ });
    await user.click(screen.getByRole("button", { name: "Overpriced" }));
    await user.selectOptions(screen.getByLabelText("Reference price"), "avg30");
    const band = screen.getByLabelText("Fair band ± (%)");
    await user.clear(band);
    await user.type(band, "7,5");
    await user.click(within(screen.getByRole("columnheader", { name: /Impact/ })).getByRole("button"));
    await waitFor(() =>
      expect(reportCalls(calls).at(-1)?.url).toBe(
        "/api/mispricing?field=avg30&threshold=7.5&filter=overpriced&sort=impact&desc=true",
      ),
    );
    const before = reportCalls(calls).length;
    await user.clear(band);
    await user.type(band, "200");
    expect(screen.getByText("Enter a band from 0 to 100 %.")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(reportCalls(calls)).toHaveLength(before);
  });

  it("is in the reports menu only with its own permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: reporter } });
    renderApp("/mispricing");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(nav).toHaveTextContent("Reports");
    expect(within(nav).queryByRole("link", { name: "Mispricing" })).not.toBeInTheDocument();
  });

  it("describes verdicts, market prices and actions", () => {
    expect(verdictText(row({ below_low: true, recently_listed: true })).text).toBe("Underpriced ▾low · new");
    expect(verdictText(row()).hint).toBe("");
    expect(marketText(row({ market: null })).text).toBe("—");
    expect(marketText(row({ condition_factor: 0.8, condition: "GD" })).hint).toContain("×0.80 for GD");
    expect(actionClass("cut_now")).toBe("action action--down action--now");
    expect(actionClass("none")).toBe("action muted");
    expect(nextMispricingSort({ sort: "priority", desc: true }, "priority")).toEqual({ sort: "priority", desc: false });
    expect(nextMispricingSort({ sort: "priority", desc: true }, "name")).toEqual({ sort: "name", desc: false });
    expect(nextMispricingSort({ sort: "priority", desc: true }, "impact")).toEqual({ sort: "impact", desc: true });
  });
});
