import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { DataInfo } from "../api/types";
import { reporter } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";

const info = (version: number): DataInfo => ({
  copy_taken_at: "2026-11-15T08:00:00Z",
  last_import: "2026-11-15",
  inventory_version: version,
  updated_at: "2026-11-15T10:30:00Z",
  test_copy: true,
});

describe("inventory updated banner", () => {
  it("appears when someone else imported and loads the new data on request", async () => {
    let version = 3;
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: reporter },
      "GET /api/data-info": () => ({ body: info(version) }),
      "POST /api/digest": { body: { since: null, last_import: "2026-11-15" } },
    });
    const { queryClient, user } = renderApp("/");
    await screen.findByRole("heading", { name: /Hello/ });
    await waitFor(() => expect(calls.some((c) => c.url === "/api/data-info")).toBe(true));
    expect(screen.queryByText(/The inventory was updated/)).not.toBeInTheDocument();

    version = 4;
    await queryClient.refetchQueries({ queryKey: ["data-info"] });
    const banner = await screen.findByRole("status");
    expect(banner).toHaveTextContent("The inventory was updated at");
    const meCalls = calls.filter((c) => c.url === "/api/auth/me").length;
    await user.click(screen.getByRole("button", { name: "Show new data" }));
    expect(screen.queryByText(/The inventory was updated/)).not.toBeInTheDocument();
    await waitFor(() => expect(calls.filter((c) => c.url === "/api/auth/me").length).toBeGreaterThan(meCalls));
  });
});
