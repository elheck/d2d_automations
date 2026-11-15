import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MarketStatusView, MoverRow, MoversView } from "../api/types";
import { admin, reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call } from "../test/server";
import { changeClass, formatChange, nextMoverSort, parseDays } from "./MoversPage";

const loaded: MarketStatusView = {
  configured: true,
  job: { state: "finished", at: "2026-11-15T08:00:00Z" },
  loaded: { version: 1, loaded_at: "2026-11-15T08:00:00Z", day: "2026-11-15", cards: 3, products: 3 },
};

function row(overrides: Partial<MoverRow> = {}): MoverRow {
  return {
    name: "Riser",
    set_code: "TST",
    condition: "NM",
    language: "English",
    is_foil: false,
    quantity: 2,
    age_days: 318,
    listed: 9,
    market: 10,
    pct_7d: 10,
    pct_30d: 30,
    location: "A-1-1-1-L2-R",
    cardmarket_url: "https://www.cardmarket.com/en/Magic/Products?idProduct=101",
    ...overrides,
  };
}

const view: MoversView = {
  version: 1,
  rows: [row(), row({ name: "Faller", pct_7d: -9.09, pct_30d: -23.1, market: 10, listed: 12, cardmarket_url: null })],
  total: 2,
};

const moverCalls = (calls: Call[]) => calls.filter((c) => c.url.startsWith("/api/movers?"));
const loads = (calls: Call[]) => calls.filter((c) => c.url === "/api/movers/load");

function server(extra: Parameters<typeof fakeServer>[0] = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: reporter },
    "GET /api/data-info": { body: { copy_taken_at: null, last_import: "2026-11-11" } },
    "GET /api/movers/status": { body: loaded },
    "POST /api/movers/load": { status: 202, body: loaded },
    "GET /api/movers": { body: view },
    ...extra,
  });
}

describe("price movers", () => {
  it("loads today's market data on opening and shows the movers", async () => {
    const { calls } = server();
    renderApp("/movers");
    const riser = await screen.findByRole("row", { name: /Riser/ });
    expect(riser).toHaveTextContent("+10.0%");
    expect(riser).toHaveTextContent("+30.0%");
    expect(riser).toHaveTextContent("318 d");
    expect(riser).toHaveTextContent("9.00 €");
    expect(within(riser).getByText("+10.0%")).toHaveClass("change--up");
    const faller = screen.getByRole("row", { name: /Faller/ });
    expect(faller).toHaveTextContent("−9.1%");
    expect(within(faller).getByText("−9.1%")).toHaveClass("change--down");
    expect(screen.getByText(/Market prices of 2026-11-15 for 3 products/)).toBeInTheDocument();
    // Exactly one automatic load, without refresh.
    expect(loads(calls).map((c) => c.body)).toEqual([{ refresh: false }]);
    expect(moverCalls(calls)[0]?.url).toBe(
      "/api/movers?field=trend&min_price=0.3&min_age=0&direction=all&sort=change7&desc=true",
    );
  });

  it("follows a running load and fetches again on request", async () => {
    let status: MarketStatusView = { configured: true, job: { state: "idle" }, loaded: null };
    let polls = 0;
    const { calls } = server({
      "GET /api/movers/status": () => {
        polls += 1;
        if (status.job.state === "running" && polls > 2) status = loaded;
        return { body: status };
      },
      "POST /api/movers/load": () => {
        status = {
          configured: true,
          job: {
            state: "running",
            started_at: "x",
            stage: "Fetching latest prices for 3 products…",
            done: null,
            total: null,
          },
          loaded: null,
        };
        return { status: 202, body: status };
      },
    });
    const { user } = renderApp("/movers");
    expect(await screen.findByText("Fetching latest prices for 3 products…")).toBeInTheDocument();
    expect(await screen.findByRole("row", { name: /Riser/ }, { timeout: 5000 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fetch again" }));
    await waitFor(() => expect(loads(calls).at(-1)?.body).toEqual({ refresh: true }));
  });

  it("explains a refused refresh but not a refused automatic load", async () => {
    const tooSoon = {
      status: 429,
      body: { error: { code: "too_soon", message: "The market data was just loaded; try again in 20 s" } },
    };
    server({ "POST /api/movers/load": tooSoon });
    const { user } = renderApp("/movers");
    await screen.findByRole("row", { name: /Riser/ });
    expect(screen.queryByText(/try again in 20 s/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Fetch again" }));
    expect(await screen.findByText(/try again in 20 s/)).toBeInTheDocument();
  });

  it("filters, sorts and checks the input", async () => {
    const { calls } = server();
    const { user } = renderApp("/movers");
    await screen.findByRole("row", { name: /Riser/ });
    await user.selectOptions(screen.getByLabelText("Price field"), "avg7");
    await user.click(screen.getByRole("button", { name: "Fallers" }));
    expect(screen.getByRole("button", { name: "Fallers" })).toHaveAttribute("aria-pressed", "true");
    const minPrice = screen.getByLabelText("Min price (€)");
    await user.clear(minPrice);
    await user.type(minPrice, "1,5");
    const minAge = screen.getByLabelText("Min age (days)");
    await user.clear(minAge);
    await user.type(minAge, "90");
    await user.click(within(screen.getByRole("columnheader", { name: /Age/ })).getByRole("button"));
    await waitFor(() =>
      expect(moverCalls(calls).at(-1)?.url).toBe(
        "/api/movers?field=avg7&min_price=1.5&min_age=90&direction=fallers&sort=age&desc=true",
      ),
    );
    const before = moverCalls(calls).length;
    await user.clear(minAge);
    await user.type(minAge, "x");
    expect(screen.getByText("Enter a whole number of days from 0 to 3650.")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(moverCalls(calls)).toHaveLength(before);
  });

  it("says when no price tracker is configured", async () => {
    const { calls } = server({
      "GET /api/movers/status": { body: { configured: false, job: { state: "idle" }, loaded: null } },
    });
    renderApp("/movers");
    expect(await screen.findByText(/No price tracker is configured/)).toBeInTheDocument();
    expect(loads(calls)).toHaveLength(0);
  });

  it("is in the reports menu", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Price movers" })).toHaveAttribute("href", "/movers");
  });

  it("formats changes and parses days", () => {
    expect(formatChange(12.345)).toBe("+12.3%");
    expect(formatChange(-4)).toBe("−4.0%");
    expect(formatChange(0)).toBe("±0.0%");
    expect(formatChange(null)).toBe("—");
    expect(changeClass(1)).toBe("num change--up");
    expect(changeClass(-1)).toBe("num change--down");
    expect(changeClass(null)).toBe("num");
    expect(nextMoverSort({ sort: "change7", desc: true }, "change7")).toEqual({ sort: "change7", desc: false });
    expect(nextMoverSort({ sort: "change7", desc: true }, "location")).toEqual({ sort: "location", desc: false });
    expect(parseDays("0")).toBe(0);
    expect(parseDays("3650")).toBe(3650);
    for (const bad of ["", "3651", "-1", "1.5", "x"]) expect(parseDays(bad), bad).toBeNull();
  });
});
