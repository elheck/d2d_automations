import { screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MeResponse, PickingView, PileRow, StockCheckView } from "../api/types";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";
import { allChosen, chosenGroups, handOver, pickingTotals, takeHandoff, wantslistFileProblem } from "./stockChecker";

const checker: MeResponse = {
  username: "checker",
  display_name: "Checker",
  permissions: ["stock_checker.use", "search.use"],
};

function pile(id: string, name: string, overrides: Partial<PileRow> = {}): PileRow {
  return {
    id,
    name,
    set_name: "Test Set",
    set_code: "TST",
    collector_number: "1",
    condition: "NM",
    language: "English",
    foil: false,
    signed: false,
    quantity: 3,
    price: 1.5,
    location: `A-0-0-${id.length}-L1-R`,
    rarity: "Common",
    comment: "",
    product_id: 1,
    cardmarket_url: null,
    ...overrides,
  };
}

const result: StockCheckView = {
  source: { version: 3, file_name: "inventory-report-Magic.csv", imported_at: "2026-11-15T10:00:00Z" },
  wanted: 5,
  found: 4,
  groups: [
    { name: "Spark", wanted: 4, found: 3, piles: [{ pile: pile("s1", "Spark"), take: 3 }] },
    { name: "Ember", wanted: 1, found: 1, piles: [{ pile: pile("e1", "Ember"), take: 1 }] },
  ],
  unparseable: ["hello"],
  unparseable_total: 1,
};

function picking(overrides: Partial<PickingView> = {}): PickingView {
  const item = (pileId: string, name: string, picked: boolean) => ({
    pile: pileId,
    quantity: 2,
    picked,
    name,
    set_name: "Test Set",
    set_code: "TST",
    collector_number: "1",
    condition: "NM",
    language: "English",
    foil: false,
    location: "A-0-0-2-L1-R",
    price: 1.5,
    image: "/api/picking/image/tst/1",
  });
  return {
    created_at: "2026-11-15T10:00:00Z",
    outdated: false,
    items: [item("s1", "Spark", false), item("e1", "Ember", true)],
    ...overrides,
  };
}

function server(routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: checker },
    "POST /api/stock-checker/check": { body: result },
    "POST /api/stock-checker/output": { body: { text: "PICKING LIST\nSpark" } },
    "POST /api/stock-checker/update-csv": { body: "csv" },
    "POST /api/picking": { body: picking() },
    "GET /api/picking": { body: picking() },
    "PUT /api/picking/picked": { body: { picked: 2 } },
    "DELETE /api/picking": { status: 204 },
    ...routes,
  });
}

const posts = (calls: Call[], path: string) => calls.filter((c) => c.method === "POST" && c.url === path);

afterEach(() => vi.unstubAllGlobals());

describe("stock checker", () => {
  it("checks a pasted list and makes outputs from the chosen piles", async () => {
    const { calls } = server();
    const { user } = renderApp("/stock-checker");
    await user.type(await screen.findByLabelText(/Wantslist or deck list/), "4 Spark{enter}1 Ember");
    await user.selectOptions(screen.getByLabelText("Preferred language"), "German");
    await user.click(screen.getByRole("button", { name: "Check stock" }));
    expect(await screen.findByText(/Found/)).toHaveTextContent("Found 4 of 5 copies.");
    expect(posts(calls, "/api/stock-checker/check")[0]?.body).toEqual({
      wantslist: "4 Spark\n1 Ember",
      language: "German",
      language_only: false,
    });
    expect(screen.getByText(/Missing: Spark ×1/)).toBeInTheDocument();
    expect(screen.getByText(/1 line is no card row: hello/)).toBeInTheDocument();

    await user.click(screen.getByLabelText("Choose Ember from A-0-0-2-L1-R"));
    expect(screen.getByText("3 copies in 1 pile chosen")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Picking list" }));
    expect(await screen.findByRole("region", { name: "Picking list" })).toHaveTextContent("PICKING LIST");
    expect(posts(calls, "/api/stock-checker/output")[0]?.body).toEqual({
      format: "picking",
      discount: 0,
      groups: [{ name: "Spark", wanted: 4, items: [{ pile: "s1", quantity: 3 }] }],
    });

    await user.clear(screen.getByLabelText("Discount %"));
    await user.type(screen.getByLabelText("Discount %"), "120");
    expect(screen.getByRole("button", { name: "Summary" })).toBeDisabled();
  });

  it("downloads the stock-update CSV and starts interactive picking", async () => {
    const { calls } = server();
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = renderApp("/stock-checker");
    await user.type(await screen.findByLabelText(/Wantslist or deck list/), "4 Spark");
    await user.click(screen.getByRole("button", { name: "Check stock" }));
    await user.click(await screen.findByRole("button", { name: "Stock-update CSV" }));
    await waitFor(() => expect(names).toEqual(["stock_update.csv"]));
    expect(posts(calls, "/api/stock-checker/update-csv")[0]?.body).toEqual({
      items: [
        { pile: "s1", quantity: 3 },
        { pile: "e1", quantity: 1 },
      ],
    });
    await user.click(screen.getByRole("button", { name: "Interactive picking" }));
    expect(await screen.findByRole("heading", { name: "Picking" })).toBeInTheDocument();
  });

  it("shows a deck-link refusal", async () => {
    server({
      "POST /api/stock-checker/check": {
        status: 422,
        body: { error: { code: "deck_link", message: "Deck links are not fetched." } },
      },
    });
    const { user } = renderApp("/stock-checker");
    await user.type(await screen.findByLabelText(/Wantslist or deck list/), "https://archidekt.com/decks/1");
    await user.click(screen.getByRole("button", { name: "Check stock" }));
    expect(await screen.findByText("Deck links are not fetched.")).toBeInTheDocument();
  });

  it("takes over a Search selection", async () => {
    server();
    handOver([{ pile: pile("s1", "Spark"), quantity: 2 }]);
    renderApp("/stock-checker");
    expect(await screen.findByText("The cards you selected in Search.")).toBeInTheDocument();
    expect(screen.getByText("2 copies in 1 pile chosen")).toBeInTheDocument();
  });

  it("rules", () => {
    expect(wantslistFileProblem({ size: 0 })).toBe("The file is empty.");
    expect(wantslistFileProblem({ size: 256 * 1024 + 1 })).toMatch(/too large/);
    handOver([
      { pile: pile("a", "Spark"), quantity: 1 },
      { pile: pile("b", "Spark"), quantity: 2 },
    ]);
    const groups = takeHandoff()!;
    expect(groups).toHaveLength(1);
    expect(groups[0]?.wanted).toBe(3);
    expect(takeHandoff()).toBeNull();
    const chosen = allChosen(result.groups);
    chosen.delete("1:e1");
    expect(chosenGroups(result.groups, chosen)).toEqual([
      { name: "Spark", wanted: 4, items: [{ pile: "s1", quantity: 3 }] },
    ]);
    expect(pickingTotals(picking().items)).toEqual({ picked: 1, total: 2, pickedValue: 3, totalValue: 6 });
  });
});

describe("picking", () => {
  it("ticks cards and shows progress and value", async () => {
    const picked = new Set(["e1"]);
    const current = () => {
      const list = picking();
      return { ...list, items: list.items.map((i) => ({ ...i, picked: picked.has(i.pile) })) };
    };
    const { calls } = server({
      "GET /api/picking": () => ({ body: current() }),
      "PUT /api/picking/picked": (call) => {
        const body = call.body as { piles: string[]; picked: boolean };
        body.piles.forEach((p) => (body.picked ? picked.add(p) : picked.delete(p)));
        return { body: { picked: picked.size } };
      },
    });
    const { user } = renderApp("/picking");
    expect(await screen.findByText("Picked 1 of 2")).toBeInTheDocument();
    expect(screen.getByText("3.00 € of 6.00 €")).toBeInTheDocument();
    const spark = screen.getByRole("article", { name: "Spark" });
    await user.click(within(spark).getByRole("button", { name: "Pick" }));
    expect(await within(spark).findByRole("button", { name: "Picked ✓" })).toBeInTheDocument();
    await waitFor(() => expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ piles: ["s1"], picked: true }));
    await user.click(screen.getByLabelText("Show picked cards"));
  });

  it("notes a changed stock and discards after asking", async () => {
    const { calls } = server({ "GET /api/picking": { body: picking({ outdated: true }) } });
    const { user } = renderApp("/picking");
    expect(await screen.findByText(/locations may have moved/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Discard list" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Discard" }));
    expect(await screen.findByText(/No picking list/)).toBeInTheDocument();
    expect(calls.some((c) => c.method === "DELETE")).toBe(true);
  });
});
