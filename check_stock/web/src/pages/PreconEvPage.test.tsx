import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { PreconStatusView } from "../api/types";
import { admin, analyst, helper } from "../test/fixtures";
import { ARMY, idle, preconSettings, ranking, row, status } from "../test/preconFixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer, type Call } from "../test/server";

const rankingCalls = (calls: Call[]) => calls.filter((c) => c.url.startsWith("/api/precon-ev/ranking"));

function server(extra: Parameters<typeof fakeServer>[0] = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: analyst },
    "GET /api/precon-ev/settings": { body: preconSettings() },
    "GET /api/precon-ev/status": { body: status() },
    "GET /api/precon-ev/ranking": { body: ranking() },
    "PUT /api/precon-ev/settings": (call) => ({ body: { settings: call.body, defaults: preconSettings().defaults } }),
    ...extra,
  });
}

describe("precon EV ranking", () => {
  it("shows the ranking with its figures", async () => {
    const { calls } = server();
    renderApp("/precon-ev");
    const army = await screen.findByRole("row", { name: /Animated Army/ });
    expect(army).toHaveTextContent("40.00 €");
    expect(army).toHaveTextContent("52.50 €");
    expect(army).toHaveTextContent("1.31×");
    expect(army).toHaveTextContent("40.25 €");
    expect(army).toHaveTextContent("98%");
    expect(within(army).getByRole("link", { name: /Animated Army/ })).toHaveAttribute("href", `/precon-ev/${ARMY}`);
    expect(screen.getByRole("row", { name: /Family Matters/ })).toHaveTextContent("not on MTGJSON yet");
    expect(screen.getByText(/2 precons · 1 with decklist · MTGJSON 5.2.2\+20261114/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
    expect(rankingCalls(calls)[0]?.url).toBe(
      "/api/precon-ev/ranking?field=trend&sealed_field=trend&cutoff=0.5&foil_needs_sales=true&sort=ratio&desc=true&filter=&hide_no_decklist=false",
    );
  });

  it("marks a stale sealed price", async () => {
    server({ "GET /api/precon-ev/ranking": { body: ranking([row({ stale_sealed: 55 })]) } });
    renderApp("/precon-ev");
    const army = await screen.findByRole("row", { name: /Animated Army/ });
    const ratio = within(army).getByText(/1.31×/);
    expect(ratio).toHaveTextContent("⚠");
    expect(ratio).toHaveAttribute("title", expect.stringContaining("55.00 €"));
  });

  it("loads the precons for everyone and follows the progress", async () => {
    let state: PreconStatusView = status({ load: idle, sales: idle, loaded: null });
    let polls = 0;
    const { calls } = server({
      "GET /api/precon-ev/status": () => {
        polls += 1;
        if (state.load.state === "running" && polls > 3) state = status();
        return { body: state };
      },
      "POST /api/precon-ev/load": () => {
        state = status({
          load: { state: "running", started_at: "x", stage: "Loading decklists…", done: 3, total: 10 },
          loaded: null,
        });
        return { status: 202, body: state };
      },
    });
    const { user } = renderApp("/precon-ev");
    expect(await screen.findByText(/Nothing loaded yet/)).toBeInTheDocument();
    expect(rankingCalls(calls)).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "Load precons" }));
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({ refresh_index: false });
    expect(await screen.findByText("Loading decklists… 3/10")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load precons" })).toBeDisabled();
    // Polling notices the end of the load; then the ranking is fetched.
    expect(await screen.findByRole("row", { name: /Animated Army/ }, { timeout: 5000 })).toBeInTheDocument();
  });

  it("asks MTGJSON again only on request", async () => {
    const { calls } = server({ "POST /api/precon-ev/load": { status: 202, body: status() } });
    const { user } = renderApp("/precon-ev");
    await user.click(await screen.findByRole("button", { name: "Refresh MTGJSON data" }));
    expect(calls.find((c) => c.method === "POST")?.body).toEqual({ refresh_index: true });
  });

  it("shows why a load cannot start", async () => {
    server({
      "POST /api/precon-ev/load": {
        status: 429,
        body: { error: { code: "too_soon", message: "The precons were just loaded; try again in 20 s" } },
      },
    });
    const { user } = renderApp("/precon-ev");
    await user.click(await screen.findByRole("button", { name: "Reload" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("try again in 20 s");
  });

  it("says when no price tracker is configured", async () => {
    server({ "GET /api/precon-ev/status": { body: status({ configured: false, loaded: null }) } });
    renderApp("/precon-ev");
    expect(await screen.findByText(/No price tracker is configured/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Load precons" })).not.toBeInTheDocument();
  });

  it("sorts on the server and saves the choice", async () => {
    const { calls } = server();
    const { user } = renderApp("/precon-ev");
    await screen.findByRole("row", { name: /Animated Army/ });
    const ev = screen.getByRole("columnheader", { name: /^EV$/ });
    await user.click(within(ev).getByRole("button"));
    await waitFor(() => expect(rankingCalls(calls).at(-1)?.url).toContain("sort=ev&desc=true"));
    expect(screen.getByRole("columnheader", { name: /^EV$/ })).toHaveAttribute("aria-sort", "descending");
    await user.click(within(screen.getByRole("columnheader", { name: /^EV$/ })).getByRole("button"));
    await waitFor(() => expect(rankingCalls(calls).at(-1)?.url).toContain("sort=ev&desc=false"));
    await user.click(within(screen.getByRole("columnheader", { name: /^Product/ })).getByRole("button"));
    await waitFor(() => expect(rankingCalls(calls).at(-1)?.url).toContain("sort=name&desc=false"));
    await waitFor(
      () => expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toMatchObject({ sort: "name", desc: false }),
      {
        timeout: 3000,
      },
    );
  });

  it("recalculates for new settings and the filter", async () => {
    const { calls } = server();
    const { user } = renderApp("/precon-ev");
    await screen.findByRole("row", { name: /Animated Army/ });
    await user.selectOptions(screen.getByLabelText("Card price"), "avg7");
    await user.selectOptions(screen.getByLabelText("Sealed price"), "low");
    await user.click(screen.getByRole("checkbox", { name: "Foil premium only if sold" }));
    await user.type(screen.getByLabelText("Filter"), "army");
    await waitFor(() =>
      expect(rankingCalls(calls).at(-1)?.url).toBe(
        "/api/precon-ev/ranking?field=avg7&sealed_field=low&cutoff=0.5&foil_needs_sales=false&sort=ratio&desc=true&filter=army&hide_no_decklist=false",
      ),
    );
    // Only the avg/trend/low columns exist for sealed products.
    const sealedOptions = within(screen.getByLabelText("Sealed price")).getAllByRole("option");
    expect(sealedOptions.map((o) => o.textContent)).toEqual(["Trend", "Average", "Low"]);
  });

  it("refuses an invalid cutoff without asking the server", async () => {
    const { calls } = server();
    const { user } = renderApp("/precon-ev");
    await screen.findByRole("row", { name: /Animated Army/ });
    const before = rankingCalls(calls).length;
    const cutoff = screen.getByLabelText("Bulk cutoff (€)");
    await user.clear(cutoff);
    await user.type(cutoff, "150");
    expect(cutoff).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText("Enter a cutoff from 0 to 100.")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(rankingCalls(calls)).toHaveLength(before);
    await user.clear(cutoff);
    await user.type(cutoff, "1,5");
    await waitFor(() => expect(rankingCalls(calls).at(-1)?.url).toContain("cutoff=1.5"));
  });

  it("starts from the user's saved settings", async () => {
    server({
      "GET /api/precon-ev/settings": { body: preconSettings({ field: "avg30", cutoff: 2, hide_no_decklist: true }) },
    });
    renderApp("/precon-ev");
    expect(await screen.findByLabelText("Card price")).toHaveValue("avg30");
    expect(screen.getByLabelText("Bulk cutoff (€)")).toHaveValue("2");
    expect(screen.getByRole("checkbox", { name: "Hide products without decklist" })).toBeChecked();
  });

  it("is only offered to users with the permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/precon-ev");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).not.toHaveTextContent("Precon EV");
  });

  it("is in the navigation and on the start page", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Precon EV" })).toHaveAttribute("href", "/precon-ev");
    expect(screen.getByRole("link", { name: /Precon EV.*sealed price/ })).toBeInTheDocument();
  });
});
