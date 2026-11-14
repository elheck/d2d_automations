import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { admin, catalog, helper } from "./test/fixtures";
import { renderApp } from "./test/renderApp";
import { fakeServer, unauthenticated } from "./test/server";

describe("app", () => {
  it("sends visitors without a session to the login page and back afterwards", async () => {
    let loggedIn = false;
    const { calls } = fakeServer({
      "GET /api/auth/me": () => (loggedIn ? { body: admin } : unauthenticated),
      "POST /api/auth/login": () => {
        loggedIn = true;
        return { body: admin };
      },
      "GET /api/permissions": { body: catalog },
      "GET /api/audit": { body: { entries: [], next_before_id: null } },
    });
    const { user, router } = renderApp("/audit");

    expect(await screen.findByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
    await user.type(screen.getByLabelText("User name"), "aron");
    await user.type(screen.getByLabelText("Password"), "a long enough password");
    await user.click(screen.getByRole("button", { name: "Log in" }));

    expect(await screen.findByRole("heading", { name: "Audit log" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/audit");
    expect(calls.find((c) => c.url === "/api/auth/login")?.body).toEqual({
      username: "aron",
      password: "a long enough password",
    });
  });

  it("shows the server's message when the login fails and clears the password", async () => {
    fakeServer({
      "GET /api/auth/me": unauthenticated,
      "POST /api/auth/login": {
        status: 401,
        body: { error: { code: "invalid_credentials", message: "Wrong user name or password" } },
      },
    });
    const { user } = renderApp("/login");
    await user.type(await screen.findByLabelText("User name"), "aron");
    await user.type(screen.getByLabelText("Password"), "wrong one");
    await user.click(screen.getByRole("button", { name: "Log in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Wrong user name or password");
    expect(screen.getByLabelText("Password")).toHaveValue("");
  });

  it("shows only the sections the user may open", async () => {
    fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(nav).toHaveTextContent("Home");
    expect(nav).not.toHaveTextContent("Users");
    expect(nav).not.toHaveTextContent("Audit log");
  });

  it("an admin sees users and the audit log", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/");
    const nav = await screen.findByRole("navigation", { name: "Main" });
    expect(nav).toHaveTextContent("Users");
    expect(nav).toHaveTextContent("Audit log");
    expect(screen.getByRole("heading", { name: "Hello, Aron" })).toBeInTheDocument();
  });

  it("pages without the permission say so instead of loading data", async () => {
    const { calls } = fakeServer({ "GET /api/auth/me": { body: helper } });
    renderApp("/users");
    expect(await screen.findByRole("heading", { name: "No access" })).toBeInTheDocument();
    expect(calls.some((c) => c.url === "/api/users")).toBe(false);
  });

  it("logging out returns to the login page", async () => {
    let loggedIn = true;
    const { calls } = fakeServer({
      "GET /api/auth/me": () => (loggedIn ? { body: admin } : unauthenticated),
      "POST /api/auth/logout": () => {
        loggedIn = false;
        return { status: 204 };
      },
    });
    const { user, router } = renderApp("/audit");
    await user.click(await screen.findByRole("button", { name: "Log out" }));
    expect(await screen.findByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/login");
    // The next person to log in starts on the start page, not on /audit.
    expect(router.state.location.search).toEqual({});
    expect(calls.some((c) => c.method === "POST" && c.url === "/api/auth/logout")).toBe(true);
  });

  it("an expired session leads to the login page", async () => {
    let sessionValid = true;
    fakeServer({
      "GET /api/auth/me": () => (sessionValid ? { body: admin } : unauthenticated),
      "GET /api/users": () => {
        sessionValid = false;
        return unauthenticated;
      },
      "GET /api/permissions": { body: catalog },
    });
    const { router } = renderApp("/users");
    await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
    expect(await screen.findByRole("button", { name: "Log in" })).toBeInTheDocument();
    // An expired session keeps the way back.
    expect(router.state.location.search).toEqual({ redirect: "/users" });
  });

  it("unknown addresses show a not-found page", async () => {
    fakeServer({ "GET /api/auth/me": { body: admin } });
    renderApp("/no/such/page");
    expect(await screen.findByRole("heading", { name: "Page not found" })).toBeInTheDocument();
  });
});
