import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { BinsView, FragmentedView, MoveListView, MoveView, PlanSummary } from "../api/types";
import { mover, reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";
import { count, exportName, groupBySource, parseWhole } from "./bins";

const source = { version: 7, file_name: "inventory-report-Magic.csv", imported_at: "2026-11-15T10:00:00Z" };

const binsView: BinsView = {
  source,
  capacity: 60,
  bins_total: 3,
  cards_total: 56,
  unplaced_rows: 2,
  rows: [
    { bin: "A-0-0-2", cards: 3, piles: 1, free: 57 },
    { bin: "A-0-0-3", cards: 3, piles: 2, free: 57 },
    { bin: "A-0-0-1", cards: 50, piles: 1, free: 10 },
  ],
};

const summary = (overrides: Partial<PlanSummary> = {}): PlanSummary => ({
  kind: "consolidation",
  threshold: 20,
  moves: 3,
  cards_moved: 6,
  bins_freed: ["A-0-0-2", "A-0-0-3"],
  total_distance: 3,
  sources_considered: 2,
  fragmented_variants: 1,
  too_large: false,
  ...overrides,
});

const fragmented: FragmentedView = {
  source,
  total: 1,
  variants: [
    {
      name: "Keeper",
      condition: "NM",
      language: "English",
      foil: false,
      copies: 51,
      bins: 2,
      piles: [
        { location: "A-0-0-1-L1-R", quantity: 50 },
        { location: "A-0-0-3-L3-R", quantity: 1 },
      ],
    },
  ],
  defrag: summary({ kind: "defrag", threshold: null, moves: 1, cards_moved: 1, bins_freed: [] }),
};

function move(id: string, name: string, from: string, to: string, overrides: Partial<MoveView> = {}): MoveView {
  return {
    id,
    name,
    local_name: null,
    set_name: "Test Set",
    set_code: "TST",
    collector_number: id,
    condition: "NM",
    language: "English",
    foil: false,
    quantity: 2,
    from_location: `${from}-L2-R`,
    to_location: `${to}-L2-R`,
    from_bin: from,
    to_bin: to,
    done: false,
    image: `/api/bins/image/tst/${id}`,
    ...overrides,
  };
}

function list(overrides: Partial<MoveListView> = {}): MoveListView {
  return {
    kind: "consolidation",
    threshold: 20,
    created_at: "2026-11-15T10:05:00Z",
    version: 7,
    outdated: false,
    source,
    summary: summary(),
    moves: [
      move("a1", "Spark", "A-0-0-2", "A-0-0-1", { local_name: "Funke", language: "German" }),
      move("b1", "Ember", "A-0-0-3", "A-0-0-1"),
      move("b2", "Keeper", "A-0-0-3", "A-0-0-1", { quantity: 1, done: true }),
    ],
    ticked: 1,
    ...overrides,
  };
}

function server(user = mover, routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: user },
    "GET /api/bins": { body: binsView },
    "GET /api/bins/consolidation": { body: summary() },
    "GET /api/bins/fragmented": { body: fragmented },
    "GET /api/bins/list": { body: list() },
    "POST /api/bins/list": { body: { list: list(), carried_ticks: 0, dropped_ticks: 0 } },
    "PUT /api/bins/list/ticks": { body: { ticked: 2 } },
    "DELETE /api/bins/list": { status: 204 },
    "GET /api/bins/list/export": { body: "csv" },
    ...routes,
  });
}

const urls = (calls: Call[], start: string) => calls.filter((c) => c.url.startsWith(start)).map((c) => c.url);

afterEach(() => vi.unstubAllGlobals());

describe("bin analysis", () => {
  it("shows the bins, the consolidation and the spread cards", async () => {
    const { calls } = server(reporter);
    const { user } = renderApp("/bins");
    const table = await screen.findByRole("table", { name: "Bins" });
    expect(within(table).getByRole("row", { name: /A-0-0-1/ })).toHaveTextContent("10");
    expect(screen.getByText(/3 bins hold 56 cards/)).toHaveTextContent("2 rows in stock have no bin location.");
    expect(screen.getByText(/From the inventory import of/)).toHaveTextContent("inventory-report-Magic.csv");
    expect(urls(calls, "/api/bins?")[0]).toBe("/api/bins?min_free=5&sort=free");

    expect(await screen.findByText(/6 cards in 3 moves; 2 bins end up empty/)).toBeInTheDocument();
    expect(screen.getByText("Emptied: A-0-0-2, A-0-0-3")).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Keeper/ })).toHaveTextContent("A-0-0-1-L1-R ×50, A-0-0-3-L3-R ×1");
    // Analysts without "Bin moves" make no lists.
    expect(screen.queryByRole("button", { name: "Make move list" })).not.toBeInTheDocument();

    await user.clear(screen.getByLabelText("Min free slots"));
    await user.type(screen.getByLabelText("Min free slots"), "20");
    await user.selectOptions(screen.getByLabelText("Sort by"), "location");
    await waitFor(() => expect(urls(calls, "/api/bins?").at(-1)).toBe("/api/bins?min_free=20&sort=location"));
    await user.clear(screen.getByLabelText("Empty bins with at most"));
    await user.type(screen.getByLabelText("Empty bins with at most"), "8");
    await waitFor(() =>
      expect(urls(calls, "/api/bins/consolidation").at(-1)).toBe("/api/bins/consolidation?threshold=8"),
    );
  });

  it("asks for an import on the server first", async () => {
    server(reporter, { "GET /api/bins": { body: { ...binsView, source: null, rows: [] } } });
    renderApp("/bins");
    expect(await screen.findByText(/needs an inventory import on the server/)).toHaveTextContent(
      "Ask someone who may import the inventory.",
    );
    expect(screen.queryByRole("heading", { name: "Empty sparse bins" })).not.toBeInTheDocument();
  });

  it("turns a plan into the user's move list", async () => {
    const { calls } = server();
    const { user } = renderApp("/bins");
    const buttons = await screen.findAllByRole("button", { name: "Make move list" });
    await user.click(buttons[0]!);
    await screen.findByRole("heading", { name: "Bin moves" });
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({ kind: "consolidation", threshold: 20 });
  });

  it("offers no list for a plan with too many moves", async () => {
    server(mover, { "GET /api/bins/consolidation": { body: summary({ moves: 6000, too_large: true }) } });
    renderApp("/bins");
    expect(await screen.findByText("That is more than 5,000 moves, too many for one list.")).toBeInTheDocument();
    expect((await screen.findAllByRole("button", { name: "Make move list" }))[0]).toBeDisabled();
  });
});

describe("bin moves", () => {
  it("groups the piles by bin and ticks them", async () => {
    // A server that remembers the ticks (the list is fetched again after one).
    const done = new Set(["b2"]);
    const current = () => {
      const moves = list().moves.map((m) => ({ ...m, done: done.has(m.id) }));
      return { ...list(), moves, ticked: done.size };
    };
    const { calls } = server(mover, {
      "GET /api/bins/list": () => ({ body: current() }),
      "PUT /api/bins/list/ticks": (call) => {
        const body = call.body as { ids: string[]; done: boolean };
        body.ids.forEach((id) => (body.done ? done.add(id) : done.delete(id)));
        return { body: { ticked: done.size } };
      },
    });
    const { user } = renderApp("/bin-moves");
    const from3 = await screen.findByRole("region", { name: "From A-0-0-3" });
    expect(from3).toHaveTextContent("2 piles · 3 cards");
    expect(screen.getByText("Moved 1 of 3")).toBeInTheDocument();
    const spark = screen.getByRole("article", { name: "Spark" });
    expect(spark).toHaveTextContent("Funke");
    expect(spark).toHaveTextContent("×2 A-0-0-2-L2-R → A-0-0-1-L2-R");
    expect(spark.querySelector("img")).toHaveAttribute("src", "/api/bins/image/tst/a1");

    await user.click(within(spark).getByRole("button", { name: "Mark moved" }));
    expect(await within(spark).findByRole("button", { name: "Moved ✓" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Moved 2 of 3")).toBeInTheDocument();
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ ids: ["a1"], done: true });
  });

  it("puts a tick back when the server refuses it", async () => {
    server(mover, {
      "PUT /api/bins/list/ticks": {
        status: 409,
        body: { error: { code: "outdated", message: "The inventory was imported again" } },
      },
    });
    const { user } = renderApp("/bin-moves");
    const spark = await screen.findByRole("article", { name: "Spark" });
    await user.click(within(spark).getByRole("button", { name: "Mark moved" }));
    expect(await screen.findByText("The inventory was imported again")).toBeInTheDocument();
    expect(within(spark).getByRole("button", { name: "Mark moved" })).toBeInTheDocument();
  });

  it("filters by bin and marks the shown piles", async () => {
    const { calls } = server();
    const { user } = renderApp("/bin-moves");
    await screen.findByRole("region", { name: "From A-0-0-3" });
    await user.selectOptions(screen.getByLabelText("Bin"), "A-0-0-3");
    expect(screen.queryByRole("region", { name: "From A-0-0-2" })).not.toBeInTheDocument();
    await user.click(screen.getByLabelText("Show moved piles"));
    expect(screen.queryByRole("article", { name: "Keeper" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Mark shown as moved" }));
    await waitFor(() => expect(calls.find((c) => c.method === "PUT")?.body).toEqual({ ids: ["b1"], done: true }));
  });

  it("downloads the moved piles and discards the list after asking", async () => {
    const { calls } = server();
    vi.stubGlobal("URL", { ...URL, createObjectURL: vi.fn(() => "blob:local/1"), revokeObjectURL: vi.fn() });
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      names.push(this.download);
    });
    const { user } = renderApp("/bin-moves");
    await user.click(await screen.findByRole("button", { name: "Download moved CSV (1)" }));
    await waitFor(() => expect(names).toEqual(["bin_consolidation_moved.csv"]));

    await user.click(screen.getByRole("button", { name: "Discard list" }));
    const dialog = screen.getByRole("dialog", { name: "Discard the move list?" });
    expect(calls.some((c) => c.method === "DELETE")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Discard" }));
    expect(await screen.findByText(/No move list yet/)).toBeInTheDocument();
    expect(calls.some((c) => c.method === "DELETE")).toBe(true);
  });

  it("makes an outdated list again on the new import", async () => {
    const { calls } = server(mover, {
      "GET /api/bins/list": { body: list({ outdated: true, moves: [], summary: null, ticked: 3 }) },
      "POST /api/bins/list": { body: { list: list(), carried_ticks: 1, dropped_ticks: 2 } },
    });
    const { user } = renderApp("/bin-moves");
    expect(await screen.findByRole("alert")).toHaveTextContent("3 ticked now");
    expect(screen.queryByRole("button", { name: /Download moved CSV/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Make the list again" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Made again: 1 ticked piles are still in the list, 2 are not (their move changed).",
    );
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({ kind: "consolidation", threshold: 20 });
  });

  it("shows the set code when there is no image", async () => {
    server();
    renderApp("/bin-moves");
    const spark = await screen.findByRole("article", { name: "Spark" });
    fireEvent.error(spark.querySelector("img")!);
    expect(spark.querySelector("img")).toBeNull();
    expect(spark).toHaveTextContent("TST");
  });

  it("is only for users with bin moves", async () => {
    fakeServer({ "GET /api/auth/me": { body: reporter } });
    renderApp("/bin-moves");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
  });

  it("parses limits and groups moves", () => {
    expect(parseWhole("20", 1, 60)).toBe(20);
    expect(parseWhole(" 0 ", 0, 60)).toBe(0);
    for (const bad of ["", "0", "61", "1.5", "-1", "x"]) expect(parseWhole(bad, 1, 60), bad).toBeNull();
    const groups = groupBySource(list().moves);
    expect(count(1, "pile")).toBe("1 pile");
    expect(count(1200, "card")).toBe("1,200 cards");
    expect(groups.map((g) => [g.bin, g.moves.length, g.cards, g.done])).toEqual([
      ["A-0-0-2", 1, 2, 0],
      ["A-0-0-3", 2, 3, 1],
    ]);
    expect(exportName("defrag")).toBe("bin_defrag_moved.csv");
  });
});
