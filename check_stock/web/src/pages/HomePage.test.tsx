import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DigestView } from "../api/types";
import { admin, helper } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { daysSince, digestParts, freshness } from "./HomePage";

const today = new Date(2026, 10, 15);

function digest(overrides: Partial<DigestView> = {}): DigestView {
  return {
    since: "2026-11-12",
    last_import: "2026-11-15",
    sold_copies: 87,
    new_listings: 12,
    bulk_lots: 1,
    restock_candidates: 356,
    sold_revenue: 101.5,
    bulk_revenue: 15,
    ...overrides,
  };
}

describe("start page digest", () => {
  it("shows what changed since the last visit", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: admin },
      "POST /api/digest": { body: digest({ last_import: null }) },
    });
    renderApp("/");
    expect(
      await screen.findByText(
        "87 copies sold (101.50 €) since 2026-11-12 · 12 new listings · 1 bulk lot (15.00 €) · 356 restock candidates",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("No inventory imported yet.")).toBeInTheDocument();
    await waitFor(() => expect(calls.filter((c) => c.url === "/api/digest")).toHaveLength(1));
    expect(calls.find((c) => c.url === "/api/digest")?.headers["X-Check-Stock-CSRF"]).toBe("1");
  });

  it("shows only the freshness to users without reports", async () => {
    fakeServer({
      "GET /api/auth/me": { body: helper },
      "POST /api/digest": {
        body: digest({
          sold_copies: null,
          new_listings: null,
          bulk_lots: null,
          restock_candidates: null,
          sold_revenue: null,
          bulk_revenue: null,
        }),
      },
    });
    renderApp("/");
    expect(await screen.findByText(/Last inventory import|Inventory imported today/)).toBeInTheDocument();
    expect(screen.queryByText(/copies sold/)).not.toBeInTheDocument();
  });

  it("words the digest like the desktop app", () => {
    expect(digestParts(digest({ sold_revenue: null, bulk_revenue: null }))).toEqual([
      "87 copies sold since 2026-11-12",
      "12 new listings",
      "1 bulk lot",
      "356 restock candidates",
    ]);
    expect(digestParts(digest({ sold_copies: 0, new_listings: 0, bulk_lots: 0, restock_candidates: 0 }))).toEqual([
      "no sales recorded since 2026-11-12",
    ]);
    expect(digestParts(digest({ since: null, restock_candidates: 0 }))).toEqual([]);
    expect(daysSince("2026-11-14", today)).toBe(1);
    expect(freshness("2026-11-15", today)).toEqual({ text: "Inventory imported today.", stale: false });
    expect(freshness("2026-11-14", today)).toEqual({ text: "Last inventory import 1 day ago.", stale: false });
    expect(freshness("2026-11-07", today).stale).toBe(true);
    expect(freshness("2026-11-08", today).stale).toBe(false);
  });
});
