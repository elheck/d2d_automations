import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { analyst } from "../test/fixtures";
import { ARMY, history, preconSettings, product, row, status } from "../test/preconFixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { productId } from "./PreconProductPage";

function server(extra: Parameters<typeof fakeServer>[0] = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: analyst },
    "GET /api/precon-ev/settings": { body: preconSettings({ field: "avg7" }) },
    "GET /api/precon-ev/status": { body: status() },
    [`GET /api/precon-ev/products/${ARMY}`]: { body: product() },
    [`GET /api/precon-ev/products/${ARMY}/history`]: { body: history() },
    ...extra,
  });
}

describe("precon EV product", () => {
  it("shows the figures, the history and every card", async () => {
    const { calls } = server();
    renderApp(`/precon-ev/${ARMY}`);
    expect(await screen.findByRole("heading", { name: /Animated Army/ })).toBeInTheDocument();
    expect(screen.getByText(/Card price Avg 7-day · sealed Trend · bulk cutoff 0.50 €/)).toBeInTheDocument();
    const sealed = screen.getByText("Sealed").closest(".kpi") as HTMLElement;
    expect(within(sealed).getByRole("link", { name: "40.00 €" })).toHaveAttribute(
      "href",
      `https://www.cardmarket.com/en/Magic/Products?idProduct=${ARMY}`,
    );
    expect(screen.getByText("EV (≥ 0.50 €)").closest(".kpi")).toHaveTextContent("52.50 €");
    expect(screen.getByText("Priced copies").closest(".kpi")).toHaveTextContent("98/100 (98%)");

    // History: the latest values in the legend, all weeks in the hidden table.
    expect(await screen.findByRole("img", { name: "EV, sealed price and Top 5 per week" })).toBeInTheDocument();
    expect(screen.getByText("EV: 52.50 €")).toBeInTheDocument();
    expect(screen.getByText("Sealed: 40.00 €")).toBeInTheDocument();
    const weeks = screen.getByRole("table", { name: "EV, sealed price and Top 5 per week" });
    expect(within(weeks).getAllByRole("row")).toHaveLength(4);

    const sol = screen.getByRole("row", { name: /Sol Ring/ });
    expect(sol).toHaveTextContent("★");
    expect(sol).toHaveTextContent("BLC #296");
    expect(sol).toHaveTextContent("9/14 d");
    expect(within(sol).getByRole("link", { name: /Sol Ring/ })).toHaveAttribute("rel", "noopener noreferrer");
    const elf = screen.getByRole("row", { name: /Cheap Elf/ });
    expect(elf).toHaveClass("row--muted");
    expect(elf).toHaveTextContent("✦");
    expect(elf).toHaveTextContent("below cutoff");
    expect(elf).toHaveTextContent("not checked");
    expect(within(elf).queryByRole("link")).not.toBeInTheDocument();

    expect(calls.find((c) => c.url.startsWith(`/api/precon-ev/products/${ARMY}?`))?.url).toBe(
      `/api/precon-ev/products/${ARMY}?field=avg7&sealed_field=trend&cutoff=0.5&foil_needs_sales=true`,
    );
  });

  it("explains products without a decklist and skips their history", async () => {
    const { calls } = server({
      [`GET /api/precon-ev/products/${ARMY}`]: {
        body: {
          version: 2,
          row: row({ state: "decklist_pending", ev: null, note: "decklist not published yet" }),
          decks: [],
        },
      },
    });
    renderApp(`/precon-ev/${ARMY}`);
    expect(await screen.findByText("decklist not published yet")).toBeInTheDocument();
    expect(calls.some((c) => c.url.includes("/history"))).toBe(false);
  });

  it("asks to load first when nothing is loaded", async () => {
    server({ "GET /api/precon-ev/status": { body: status({ loaded: null }) } });
    renderApp(`/precon-ev/${ARMY}`);
    expect(await screen.findByText(/The precons are not loaded yet/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Load them on the ranking page." })).toHaveAttribute("href", "/precon-ev");
  });

  it("shows when the history cannot be fetched", async () => {
    server({
      [`GET /api/precon-ev/products/${ARMY}/history`]: {
        status: 502,
        body: { error: { code: "tracker_unavailable", message: "The price tracker did not answer" } },
      },
    });
    renderApp(`/precon-ev/${ARMY}`);
    expect(await screen.findByText("The price tracker did not answer")).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /Sol Ring/ })).toBeInTheDocument();
  });

  it("only accepts numeric ids", () => {
    expect(productId("774134")).toBe(774134);
    for (const bad of [undefined, "", "abc", "12a", "-1", "1".repeat(13)]) {
      expect(productId(bad), String(bad)).toBeNull();
    }
  });
});
