import { fireEvent, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { admin, helper, lister, singleCard } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { DEFAULT_SET_KEY } from "./singles";

afterEach(() => window.localStorage.clear());

describe("singles listing", () => {
  it("looks a card up and gets ready for the next one", async () => {
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": { body: singleCard() },
    });
    const { user } = renderApp("/singles");
    const field = await screen.findByLabelText("Card");
    expect(field).toHaveFocus();

    await user.type(field, "HOU120");
    expect(screen.getByText("→ HOU #120")).toBeInTheDocument();
    await user.keyboard("{Enter}");

    const card = await screen.findByRole("article", { name: "Lightning Bolt" });
    expect(within(card).getByText("Hour of Devastation (HOU) #120 · Common")).toBeInTheDocument();
    expect(within(card).getByText("Mana cost: {R}")).toBeInTheDocument();
    expect(within(card).getByText("EUR 0.25 € · EUR foil 1.10 €")).toBeInTheDocument();
    const image = within(card).getByRole("img", { name: "Lightning Bolt (HOU #120)" });
    expect(image).toHaveAttribute("src", "/api/singles/image/hou/120");

    const trend = within(card).getByRole("row", { name: /Trend/ });
    expect(trend).toHaveTextContent("0.28 €");
    expect(trend).toHaveTextContent("1.05 €");
    expect(within(card).getByRole("row", { name: /^Low/ })).toHaveTextContent("—");
    expect(within(card).getByText("Prices of 2026-11-13 from the price tracker.")).toBeInTheDocument();

    const link = within(card).getByRole("link", { name: "Open on Cardmarket" });
    expect(link).toHaveAttribute("href", "https://www.cardmarket.com/en/Magic/Products?idProduct=301234");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");

    const request = calls.find((c) => c.url.startsWith("/api/singles/lookup"));
    expect(request?.url).toBe("/api/singles/lookup?input=HOU120&default_set=");
    // Ready for the next card: empty, focused, set remembered.
    expect(field).toHaveValue("");
    expect(field).toHaveFocus();
    expect(screen.getByLabelText("Default set")).toHaveValue("hou");
    expect(window.localStorage.getItem(DEFAULT_SET_KEY)).toBe("hou");
  });

  it("takes only the number when a default set is given", async () => {
    window.localStorage.setItem(DEFAULT_SET_KEY, "hou");
    const { calls } = fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": { body: singleCard({ collector_number: "7", name: "Seventh" }) },
    });
    const { user } = renderApp("/singles");
    expect(await screen.findByLabelText("Default set")).toHaveValue("hou");
    const field = screen.getByLabelText("Card");
    expect(field).toHaveAttribute("placeholder", "e.g. 120");
    await user.type(field, "007{Enter}");
    expect(await screen.findByRole("article", { name: "Seventh" })).toBeInTheDocument();
    expect(calls.some((c) => c.url === "/api/singles/lookup?input=007&default_set=hou")).toBe(true);
  });

  it("explains malformed input without asking the server", async () => {
    const { calls } = fakeServer({ "GET /api/auth/me": { body: lister } });
    const { user } = renderApp("/singles");
    const field = await screen.findByLabelText("Card");
    await user.type(field, "hou12a{Enter}");
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByText(/Enter a set code and a three-digit number/)).toBeInTheDocument();
    expect(field).toHaveValue("hou12a");
    expect(calls.some((c) => c.url.startsWith("/api/singles"))).toBe(false);
  });

  it("shows the server's error and which card it was", async () => {
    fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": {
        status: 404,
        body: { error: { code: "card_not_found", message: "Scryfall knows no card with this set code" } },
      },
    });
    const { user } = renderApp("/singles");
    await user.type(await screen.findByLabelText("Card"), "hou999{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Scryfall knows no card with this set code");
    expect(screen.getByText("HOU #999")).toBeInTheDocument();
  });

  it("says why prices are missing and has no image or link without data", async () => {
    fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": {
        body: singleCard({
          image_path: null,
          cardmarket_id: null,
          cardmarket_url: null,
          prices: { status: "no_cardmarket_id" },
        }),
      },
    });
    const { user } = renderApp("/singles");
    await user.type(await screen.findByLabelText("Card"), "hou120{Enter}");
    const card = await screen.findByRole("article", { name: "Lightning Bolt" });
    expect(within(card).getByText("No image available")).toBeInTheDocument();
    expect(within(card).queryByRole("img")).not.toBeInTheDocument();
    expect(within(card).queryByRole("link")).not.toBeInTheDocument();
    expect(within(card).getByText(/Scryfall knows no Cardmarket product/)).toBeInTheDocument();
  });

  it("never links anywhere but Cardmarket", async () => {
    fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": { body: singleCard({ cardmarket_url: "javascript:alert(1)" }) },
    });
    const { user } = renderApp("/singles");
    await user.type(await screen.findByLabelText("Card"), "hou120{Enter}");
    const card = await screen.findByRole("article", { name: "Lightning Bolt" });
    expect(within(card).getByText(/Product 301234/)).toBeInTheDocument();
    expect(within(card).queryByRole("link")).not.toBeInTheDocument();
  });

  it("falls back to text when the image cannot be loaded", async () => {
    fakeServer({
      "GET /api/auth/me": { body: lister },
      "GET /api/singles/lookup": { body: singleCard() },
    });
    const { user } = renderApp("/singles");
    await user.type(await screen.findByLabelText("Card"), "hou120{Enter}");
    const image = await screen.findByRole("img");
    fireEvent.error(image);
    expect(await screen.findByText("No image available")).toBeInTheDocument();
  });

  it("is only offered to users with the permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/singles");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Main" })).not.toHaveTextContent("Singles");
  });

  it("is in the navigation and on the start page for users with the permission", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(within(nav).getByRole("link", { name: "Singles" })).toHaveAttribute("href", "/singles");
    expect(screen.getByRole("link", { name: /Singles.*Look up cards/ })).toBeInTheDocument();
  });
});
