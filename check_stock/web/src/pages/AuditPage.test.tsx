import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { admin, auditPage, catalog } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { formatDetails } from "./AuditPage";

describe("audit page", () => {
  it("shows readable entries and loads older ones", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: admin },
      "GET /api/permissions": { body: catalog },
      "GET /api/audit?limit=50": { body: auditPage([4, 3], 3) },
      "GET /api/audit?limit=50&before_id=3": { body: auditPage([2, 1], null) },
    });
    const { user } = renderApp("/audit");
    expect(await screen.findAllByText("Logged in")).toHaveLength(1);
    expect(screen.getByText("User changed")).toBeInTheDocument();
    expect(await screen.findByText("permissions: added: View audit log; removed: none")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Load older entries" }));
    await waitFor(() => expect(screen.getAllByText("Logged in")).toHaveLength(2));
    expect(calls.some((c) => c.url === "/api/audit?limit=50&before_id=3")).toBe(true);
    expect(screen.queryByRole("button", { name: "Load older entries" })).not.toBeInTheDocument();
  });

  it("formats details compactly", () => {
    expect(formatDetails({})).toBe("");
    expect(formatDetails(null)).toBe("");
    expect(formatDetails({ ip: "10.0.0.1", reason: "wrong_password" })).toBe("ip: 10.0.0.1; reason: wrong password");
    expect(formatDetails({ display_name: { from: "A", to: "B" } })).toBe("display name: from: A; to: B");
    expect(formatDetails({ permissions: ["a", "b"] })).toBe("permissions: a, b");
    const labels = new Map([["search.use", "Search cards"]]);
    expect(formatDetails({ permissions: ["search.use", "future.thing"] }, labels)).toBe(
      "permissions: Search cards, future.thing",
    );
    expect(formatDetails({ reason: "unknown_user", ip: "10.0.0.1" })).toBe("reason: unknown user; ip: 10.0.0.1");
    expect(formatDetails({ reason: "something_new" })).toBe("reason: something_new");
  });
});
