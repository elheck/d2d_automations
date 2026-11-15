import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { BuyHelperSettings, BuyOffer, BuyOfferParams } from "../api/types";
import { admin, buyer, helper } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call } from "../test/server";

const defaults: BuyOfferParams = {
  single_common: false,
  single_uncommon: false,
  single_rare: true,
  single_mythic: true,
  min_single_price: 0.5,
  single_buy_percent: 60,
  bulk_rate: 10,
  bulk_batch: 1000,
};

const settings = (params: BuyOfferParams = defaults): BuyHelperSettings => ({ params, defaults });

function offer(overrides: Partial<BuyOffer["summary"]> = {}): BuyOffer {
  return {
    rows: 4,
    summary: {
      single_rows: 2,
      single_cards: 3,
      single_market_value: 15001.5,
      single_offer: 9000.9,
      bulk_rows: 2,
      bulk_cards: 1004,
      bulk_market_value: 101,
      bulk_offer: 10.04,
      total_offer: 9010.94,
      ...overrides,
    },
    singles: [
      { name: "Black Lotus", set_code: "LEA", rarity: "rare", quantity: 1, unit_price: 15000, offer: 9000 },
      { name: "Counterspell", set_code: "LEA", rarity: "common", quantity: 2, unit_price: 0.75, offer: 0.9 },
    ],
    singles_total: 2,
  };
}

const CSV = "cardmarketId,quantity,name,condition,language,price\n1,1,Black Lotus,NM,EN,15000\n";

function server(extra: Record<string, Parameters<typeof fakeServer>[0][string]> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: buyer },
    "GET /api/buy-helper/settings": { body: settings() },
    "POST /api/buy-helper/offer": { body: offer() },
    "PUT /api/buy-helper/settings": (call: Call) => ({ body: settings(call.body as BuyOfferParams) }),
    ...extra,
  });
}

async function openWithFile(csv = CSV) {
  const view = renderApp("/buy-helper");
  const input = await screen.findByLabelText(/Seller's card list/);
  await view.user.upload(input, new File([csv], "seller.csv", { type: "text/csv" }));
  return view;
}

const offerCalls = (calls: Call[]) => calls.filter((c) => c.url === "/api/buy-helper/offer");
const saveCalls = (calls: Call[]) => calls.filter((c) => c.method === "PUT");

afterEach(() => vi.restoreAllMocks());

describe("buy helper", () => {
  it("values an uploaded card list with the saved settings", async () => {
    const { calls } = server();
    await openWithFile();
    const summary = await screen.findByRole("row", { name: /^Total/ });
    expect(summary).toHaveTextContent("1,007");
    expect(summary).toHaveTextContent("9010.94 €");
    expect(screen.getByRole("row", { name: /^Bulk/ })).toHaveTextContent("10.04 €");
    expect(screen.getByRole("row", { name: /Black Lotus/ })).toHaveTextContent("9000.00 €");
    expect(screen.getByText("seller.csv · 4 card rows")).toBeInTheDocument();
    expect(screen.getByText("Choose another list…")).toBeInTheDocument();

    const [first] = offerCalls(calls);
    expect(first?.body).toEqual({ csv: CSV, params: defaults });
    expect(first?.headers["X-Check-Stock-CSRF"]).toBe("1");
    // Nothing changed, so nothing is saved.
    expect(saveCalls(calls)).toHaveLength(0);
  });

  it("recalculates and saves when a setting changes", async () => {
    const { calls } = server();
    const { user } = await openWithFile();
    await screen.findByRole("row", { name: /^Total/ });

    const share = screen.getByLabelText("Share of a single's value (%)");
    await user.clear(share);
    await user.type(share, "50");
    await user.click(screen.getByRole("checkbox", { name: "Common" }));
    const expected = { ...defaults, single_buy_percent: 50, single_common: true };

    await waitFor(() => expect(offerCalls(calls).at(-1)?.body).toEqual({ csv: CSV, params: expected }));
    await waitFor(() => expect(saveCalls(calls).at(-1)?.body).toEqual(expected), { timeout: 3000 });
    expect(await screen.findByText("Saved for your account")).toBeInTheDocument();
    // Typing "5", then "50" settles into one save, not one per key.
    expect(saveCalls(calls)).toHaveLength(1);
  });

  it("accepts a decimal comma", async () => {
    const { calls } = server();
    const { user } = await openWithFile();
    await screen.findByRole("row", { name: /^Total/ });
    const threshold = screen.getByLabelText("Single price threshold (€)");
    await user.clear(threshold);
    await user.type(threshold, "0,25");
    await waitFor(() =>
      expect(offerCalls(calls).at(-1)?.body).toEqual({ csv: CSV, params: { ...defaults, min_single_price: 0.25 } }),
    );
  });

  it("explains invalid settings and neither calculates nor saves them", async () => {
    const { calls } = server();
    const { user } = await openWithFile();
    await screen.findByRole("row", { name: /^Total/ });
    const before = offerCalls(calls).length;

    const share = screen.getByLabelText("Share of a single's value (%)");
    await user.clear(share);
    await user.type(share, "150");
    expect(share).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Enter a share from 0 to 100.")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 1000));
    expect(offerCalls(calls)).toHaveLength(before);
    expect(saveCalls(calls)).toHaveLength(0);
  });

  it("starts from the user's saved settings and can go back to the defaults", async () => {
    const mine = { ...defaults, single_buy_percent: 45, min_single_price: null, bulk_batch: 500 };
    const { calls } = server({ "GET /api/buy-helper/settings": { body: settings(mine) } });
    const { user } = renderApp("/buy-helper");
    expect(await screen.findByLabelText("Share of a single's value (%)")).toHaveValue("45");
    expect(screen.getByRole("checkbox", { name: "Cards worth at least" })).not.toBeChecked();
    expect(screen.getByLabelText("Single price threshold (€)")).toBeDisabled();
    expect(screen.getByLabelText("Bulk batch (cards)")).toHaveValue("500");

    await user.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect(screen.getByLabelText("Share of a single's value (%)")).toHaveValue("60");
    await waitFor(() => expect(saveCalls(calls).at(-1)?.body).toEqual(defaults), { timeout: 3000 });
  });

  it("saves a change that was not saved yet when the page is left", async () => {
    const { calls } = server();
    const { user } = renderApp("/buy-helper");
    const share = await screen.findByLabelText("Share of a single's value (%)");
    await user.clear(share);
    await user.type(share, "55");
    await user.click(screen.getByRole("link", { name: "Home" }));
    await waitFor(() => expect(saveCalls(calls).at(-1)?.body).toEqual({ ...defaults, single_buy_percent: 55 }));
  });

  it("sends unsaved changes when the tab is closed or reloaded", async () => {
    const { calls } = server();
    const { user } = renderApp("/buy-helper");
    const share = await screen.findByLabelText("Share of a single's value (%)");
    await user.clear(share);
    await user.type(share, "57");
    window.dispatchEvent(new Event("pagehide"));
    const saves = saveCalls(calls);
    expect(saves).toHaveLength(1);
    expect(saves[0]?.body).toEqual({ ...defaults, single_buy_percent: 57 });
    expect(saves[0]?.keepalive).toBe(true);
    // Already sent: the regular save does not repeat it.
    await new Promise((resolve) => setTimeout(resolve, 1200));
    expect(saveCalls(calls)).toHaveLength(1);
  });

  it("refuses files that are too large without sending them", async () => {
    const { calls } = server();
    const { user } = renderApp("/buy-helper");
    const input = await screen.findByLabelText(/Seller's card list/);
    const big = new File(["x"], "huge.csv", { type: "text/csv" });
    Object.defineProperty(big, "size", { value: 6 * 1024 * 1024 + 1 });
    await user.upload(input, big);
    expect(await screen.findByRole("alert")).toHaveTextContent("The file is too large (at most 6 MB).");
    expect(offerCalls(calls)).toHaveLength(0);
  });

  it("shows why the server cannot read a file", async () => {
    server({
      "POST /api/buy-helper/offer": {
        status: 400,
        body: { error: { code: "invalid_csv", message: "This is not a card export (line 2 cannot be read)" } },
      },
    });
    await openWithFile("nonsense");
    expect(await screen.findByRole("alert")).toHaveTextContent("This is not a card export (line 2 cannot be read)");
  });

  it("downloads the offer as a CSV", async () => {
    const { calls } = server({ "POST /api/buy-helper/export": { body: "csv" } });
    const created = vi.fn((_blob: Blob) => "blob:local/offer");
    vi.stubGlobal("URL", { ...URL, createObjectURL: created, revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = await openWithFile();
    await user.click(await screen.findByRole("button", { name: "Download offer CSV" }));
    await waitFor(() => expect(names).toEqual(["buy_offer.csv"]));
    const exported = calls.find((c) => c.url === "/api/buy-helper/export");
    expect(exported?.body).toEqual({ csv: CSV, params: defaults });
    expect(exported?.headers["X-Check-Stock-CSRF"]).toBe("1");
    vi.unstubAllGlobals();
  });

  it("lists only what the server sent and says when there are more singles", async () => {
    const many = offer();
    many.singles_total = 250;
    server({ "POST /api/buy-helper/offer": { body: many } });
    await openWithFile();
    expect(await screen.findByText(/Showing the 2 most valuable of 250 singles/)).toBeInTheDocument();
    const table = screen.getByRole("row", { name: /Counterspell/ }).closest("table") as HTMLElement;
    expect(within(table).getAllByRole("row")).toHaveLength(3);
  });

  it("is only offered to users with the permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/buy-helper");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).not.toHaveTextContent("Buy helper");
  });

  it("is in the navigation and on the start page", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Buy helper" })).toHaveAttribute("href", "/buy-helper");
    expect(screen.getByRole("link", { name: /Buy helper.*seller's card list/ })).toBeInTheDocument();
  });
});
