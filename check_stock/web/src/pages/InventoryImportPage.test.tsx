import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DataInfo, InventoryImportResult, InventoryPreview, InventoryVersion } from "../api/types";
import { helper, importer } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call, type Route } from "../test/server";
import { importFileProblem, MAX_IMPORT_BYTES, paceText, typedCount, unusualText } from "./inventoryImport";

const CSV = "cardmarketId,quantity,name,condition,language,price\n1,1,Bolt,NM,English,1.00\n";

const dataInfo: DataInfo = {
  copy_taken_at: "2026-11-15T08:00:00Z",
  last_import: "2026-11-11",
  inventory_version: 3,
  updated_at: "2026-11-15T08:00:00Z",
  test_copy: true,
};

function preview(overrides: Partial<InventoryPreview> = {}): InventoryPreview {
  return {
    file_name: "inventory-report-Magic.csv",
    category: "Magic",
    new_category: false,
    rows: 16_000,
    copies_in_file: 29_300,
    numbers: {
      new_variants: 12,
      updated_variants: 15_988,
      zeroed_variants: 40,
      copies_sold: 119,
      bulk_removed: 0,
      price_changes: 230,
      copies_before: 29_419,
      copies_after: 29_300,
    },
    blocked: false,
    pace: { daily_copies: 21.6, days: 30, last_import: "2026-11-11" },
    unusual: null,
    expires_at: "2026-11-15T09:00:00Z",
    valid_minutes: 30,
    ...overrides,
  };
}

const result: InventoryImportResult = {
  version: 4,
  category: "Magic",
  numbers: preview().numbers,
  forced: false,
  upserted: 16_000,
  zeroed: 40,
  bulk_removed: 0,
};

const history: InventoryVersion[] = [
  {
    id: 3,
    category: "Magic",
    created_at: "2026-11-15T08:00:00Z",
    kind: "desktop_copy",
    user_name: "command line",
    file_rows: 0,
    copies_sold: 0,
    new_variants: 0,
    zeroed_variants: 0,
  },
];

function server(routes: Record<string, Route> = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: importer },
    "GET /api/data-info": { body: dataInfo },
    "GET /api/inventory/imports": { body: history },
    "POST /api/inventory/preview": { body: preview() },
    "POST /api/inventory/confirm": { body: result },
    "DELETE /api/inventory/pending": { status: 204 },
    ...routes,
  });
}

const posts = (calls: Call[], path: string) => calls.filter((c) => c.method === "POST" && c.url === path);

async function chooseFile(user: ReturnType<typeof renderApp>["user"], content = CSV) {
  const file = new File([content], "inventory-report-Magic.csv", { type: "text/csv" });
  await user.upload(await screen.findByLabelText("Cardmarket inventory export (CSV)"), file);
}

describe("inventory import", () => {
  it("checks the file first and imports it only on confirm", async () => {
    const { calls } = server();
    const { user } = renderApp("/inventory-import");
    expect(await screen.findByText("Test copy.")).toBeInTheDocument();
    await chooseFile(user);
    const check = await screen.findByRole("region", { name: "Check of the file" });
    expect(posts(calls, "/api/inventory/preview")[0]?.body).toEqual({
      file_name: "inventory-report-Magic.csv",
      csv: CSV,
    });
    expect(posts(calls, "/api/inventory/confirm")).toHaveLength(0);
    expect(within(check).getByRole("row", { name: /Copies that will count as sold/ })).toHaveTextContent("119");
    expect(within(check).getByRole("row", { name: /Copies in stock/ })).toHaveTextContent("29,419 → 29,300");
    expect(within(check).getByText(/Usually about 22 copies sell per day/)).toBeInTheDocument();
    expect(within(check).queryByRole("alert")).not.toBeInTheDocument();

    await user.click(within(check).getByRole("button", { name: "Import" }));
    expect(await screen.findByText(/Imported \(Magic\)/)).toBeInTheDocument();
    expect(posts(calls, "/api/inventory/confirm")[0]?.body).toEqual({
      file_name: "inventory-report-Magic.csv",
      csv: CSV,
    });
    expect(screen.queryByRole("region", { name: "Check of the file" })).not.toBeInTheDocument();
    // Every page's data is loaded again, the user's own import raises no banner.
    await waitFor(() => expect(calls.filter((c) => c.url === "/api/inventory/imports").length).toBe(2));
    expect(screen.queryByText(/The inventory was updated/)).not.toBeInTheDocument();
  });

  it("warns about unusually many sales but still allows the import", async () => {
    server({
      "POST /api/inventory/preview": {
        body: preview({ unusual: { daily_copies: 21.6, days_since_last_import: 1, expected_copies: 21.6 } }),
      },
    });
    const { user } = renderApp("/inventory-import");
    await chooseFile(user);
    const check = await screen.findByRole("region", { name: "Check of the file" });
    expect(within(check).getByRole("alert")).toHaveTextContent(
      "119 copies would count as sold: more than 3× the usual. The last import was 1 day ago; about 22 copies would be expected.",
    );
    expect(within(check).getByRole("button", { name: "Import" })).toBeEnabled();
  });

  it("imports a blocked file only with the typed number of copies", async () => {
    const blocked = preview({
      blocked: true,
      numbers: { ...preview().numbers, copies_sold: 20_000, copies_after: 9_419 },
    });
    const { calls } = server({ "POST /api/inventory/preview": { body: blocked } });
    const { user } = renderApp("/inventory-import");
    await chooseFile(user);
    const check = await screen.findByRole("region", { name: "Check of the file" });
    expect(within(check).getByRole("alert")).toHaveTextContent(
      "Blocked: this import would count 20,000 of 29,419 copies, more than half the stock, as sold.",
    );
    expect(within(check).queryByRole("button", { name: "Import" })).not.toBeInTheDocument();
    const anyway = within(check).getByRole("button", { name: "Import anyway" });
    expect(anyway).toBeDisabled();
    const count = within(check).getByLabelText("Copies that will count as sold");
    await user.type(count, "2000");
    expect(anyway).toBeDisabled();
    await user.type(count, "0");
    expect(anyway).toBeEnabled();
    await user.click(anyway);
    await waitFor(() => expect(posts(calls, "/api/inventory/confirm")).toHaveLength(1));
    expect(posts(calls, "/api/inventory/confirm")[0]?.body).toMatchObject({ confirm_sold: "20000" });
  });

  it("offers to check again when the data changed meanwhile", async () => {
    const { calls } = server({
      "POST /api/inventory/confirm": {
        status: 409,
        body: { error: { code: "outdated", message: "The inventory was changed since the check (another import)" } },
      },
    });
    const { user } = renderApp("/inventory-import");
    await chooseFile(user);
    await user.click(await screen.findByRole("button", { name: "Import" }));
    expect(await screen.findByText(/changed since the check/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Check the file again" }));
    await waitFor(() => expect(posts(calls, "/api/inventory/preview")).toHaveLength(2));
  });

  it("cancels the check on the server", async () => {
    const { calls } = server();
    const { user } = renderApp("/inventory-import");
    await chooseFile(user);
    const check = await screen.findByRole("region", { name: "Check of the file" });
    await user.click(within(check).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(calls.some((c) => c.method === "DELETE")).toBe(true));
    expect(screen.queryByRole("region", { name: "Check of the file" })).not.toBeInTheDocument();
  });

  it("refuses files that are too large without sending them", async () => {
    const { calls } = server();
    const { user } = renderApp("/inventory-import");
    const big = new File(["x"], "inventory-report-Magic.csv");
    Object.defineProperty(big, "size", { value: MAX_IMPORT_BYTES + 1 });
    await user.upload(await screen.findByLabelText("Cardmarket inventory export (CSV)"), big);
    expect(await screen.findByText("The file is too large (at most 16 MB).")).toBeInTheDocument();
    expect(posts(calls, "/api/inventory/preview")).toHaveLength(0);
  });

  it("lists the recent changes", async () => {
    server();
    renderApp("/inventory-import");
    const row = await screen.findByRole("row", { name: /Copy of the desktop data/ });
    expect(row).toHaveTextContent("command line");
  });

  it("is only for users who may import", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/inventory-import");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).not.toHaveTextContent("Inventory import");
  });

  it("reads typed counts and file sizes like the server", () => {
    expect(typedCount("20000")).toBe(20000);
    expect(typedCount(" 20,000 ")).toBe(20000);
    expect(typedCount("20.000")).toBe(20000);
    for (const bad of ["", "abc", "-5", "1e3"]) expect(typedCount(bad), bad).toBeNull();
    expect(importFileProblem({ name: "a.csv", size: 0 })).toBe("The file is empty.");
    expect(importFileProblem({ name: "a.csv", size: MAX_IMPORT_BYTES })).toBeNull();
    expect(paceText(null)).toMatch(/not enough sales history/);
    expect(paceText({ daily_copies: 2.25, days: 9, last_import: "2026-03-10" })).toBe(
      "Usually about 2.3 copies sell per day (over the 9 days up to 2026-03-10).",
    );
    expect(unusualText({ daily_copies: 1, days_since_last_import: 3, expected_copies: 3 }, 1)).toMatch(
      /^1 copy would count as sold.*3 days ago; about 3 copies/,
    );
  });
});
