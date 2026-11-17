import { screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { admin, catalog, userView } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";
import { userChanges } from "./UsersPage";

const aron = userView();
const helperUser = userView({
  id: 2,
  username: "helper",
  display_name: "Helper",
  permissions: ["search.use"],
  last_login_at: null,
});

function server(extra = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: admin },
    "GET /api/permissions": { body: catalog },
    "GET /api/users": { body: [aron, helperUser] },
    ...extra,
  });
}

async function openUsers() {
  const app = renderApp("/users");
  await screen.findByRole("cell", { name: "helper" });
  return app;
}

/** The table row of a login name (the own row's cell also holds the "you" badge). */
function row(name: string) {
  return screen.getByRole("cell", { name: new RegExp(`^${name}\\s*(you)?$`) }).closest("tr") as HTMLElement;
}

describe("users page", () => {
  it("lists users with status and permission count", async () => {
    server();
    await openUsers();
    expect(within(row("helper")).getByText(`1 of ${catalog.length}`)).toBeInTheDocument();
    expect(within(row("helper")).getByText("Active")).toBeInTheDocument();
    expect(within(screen.getByText("you").closest("tr") as HTMLElement).getByText("All")).toBeInTheDocument();
  });

  it("shows locked and disabled accounts", async () => {
    fakeServer({
      "GET /api/auth/me": { body: admin },
      "GET /api/permissions": { body: catalog },
      "GET /api/users": {
        body: [
          aron,
          userView({ id: 3, username: "locked", locked_until: "2026-11-14T09:00:00Z" }),
          userView({ id: 4, username: "gone", disabled: true }),
        ],
      },
    });
    renderApp("/users");
    await screen.findByRole("cell", { name: "locked" });
    expect(within(row("locked")).getByText(/^Locked until/)).toBeInTheDocument();
    expect(within(row("gone")).getByText("Disabled")).toBeInTheDocument();
  });

  it("creates a user with the chosen permissions, showing their descriptions", async () => {
    const { calls } = server({ "POST /api/users": { status: 201, body: helperUser } });
    const { user } = await openUsers();
    await user.click(screen.getByRole("button", { name: "New user" }));
    const dialog = screen.getByRole("dialog", { name: "New user" });
    // The owner wants each permission's description in the admin interface.
    for (const p of catalog) {
      expect(within(dialog).getByText(p.description, { exact: false })).toBeInTheDocument();
    }
    await user.type(within(dialog).getByLabelText(/Login name/), "Ben");
    await user.type(within(dialog).getByLabelText("Display name"), "Ben Becker");
    await user.type(within(dialog).getByLabelText(/^Password/), "bens long password");
    await user.type(within(dialog).getByLabelText("Repeat password"), "bens long password");
    await user.click(within(dialog).getByRole("checkbox", { name: "Discard cards" }));
    await user.click(within(dialog).getByRole("button", { name: "Create user" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(calls.find((c) => c.method === "POST" && c.url === "/api/users")?.body).toEqual({
      username: "Ben",
      display_name: "Ben Becker",
      password: "bens long password",
      permissions: ["search.use", "inventory.discard"],
    });
  });

  it("checks the password before sending", async () => {
    const { calls } = server();
    const { user } = await openUsers();
    await user.click(screen.getByRole("button", { name: "New user" }));
    const dialog = screen.getByRole("dialog", { name: "New user" });
    await user.type(within(dialog).getByLabelText(/Login name/), "ben");
    await user.type(within(dialog).getByLabelText("Display name"), "Ben");
    await user.type(within(dialog).getByLabelText(/^Password/), "short");
    await user.type(within(dialog).getByLabelText("Repeat password"), "short");
    await user.click(within(dialog).getByRole("button", { name: "Create user" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("at least 12 characters");
    expect(calls.some((c) => c.method === "POST")).toBe(false);
  });

  it("shows the server's refusal inside the dialog", async () => {
    server({
      "PATCH /api/users/1": {
        status: 409,
        body: {
          error: { code: "last_admin", message: 'at least one enabled user must keep the "Manage users" permission' },
        },
      },
    });
    const { user } = await openUsers();
    await user.click(within(row("aron")).getByRole("button", { name: "Edit" }));
    const dialog = screen.getByRole("dialog", { name: "Edit aron" });
    await user.click(within(dialog).getByRole("checkbox", { name: "Manage users" }));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("Manage users");
  });

  it("edits send only what changed", async () => {
    const { calls } = server({ "PATCH /api/users/2": { body: helperUser } });
    const { user } = await openUsers();
    await user.click(within(row("helper")).getByRole("button", { name: "Edit" }));
    const dialog = screen.getByRole("dialog", { name: "Edit helper" });
    expect(within(dialog).getByRole("button", { name: "Save" })).toBeDisabled();
    await user.click(within(dialog).getByRole("checkbox", { name: "View audit log" }));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(calls.find((c) => c.method === "PATCH")?.body).toEqual({ permissions: ["audit.view", "search.use"] });
  });

  it("does not offer to disable or delete your own account", async () => {
    server();
    const { user } = await openUsers();
    expect(within(row("aron")).getByRole("button", { name: "Delete" })).toBeDisabled();
    expect(within(row("helper")).getByRole("button", { name: "Delete" })).toBeEnabled();
    await user.click(within(row("aron")).getByRole("button", { name: "Edit" }));
    expect(within(screen.getByRole("dialog")).getByRole("checkbox", { name: /Account disabled/ })).toBeDisabled();
  });

  it("deletes after confirming", async () => {
    const { calls } = server({ "DELETE /api/users/2": { status: 204 } });
    const { user } = await openUsers();
    await user.click(within(row("helper")).getByRole("button", { name: "Delete" }));
    const dialog = screen.getByRole("dialog", { name: "Delete helper?" });
    expect(calls.some((c) => c.method === "DELETE")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "Delete user" }));
    await waitFor(() => expect(calls.some((c) => c.method === "DELETE" && c.url === "/api/users/2")).toBe(true));
  });

  it("sets a password", async () => {
    const { calls } = server({ "POST /api/users/2/password": { status: 204 } });
    const { user } = await openUsers();
    await user.click(within(row("helper")).getByRole("button", { name: "Password" }));
    const dialog = screen.getByRole("dialog", { name: "New password for helper" });
    await user.type(within(dialog).getByLabelText(/^New password/), "a new long password");
    await user.type(within(dialog).getByLabelText("Repeat password"), "a new long password");
    await user.click(within(dialog).getByRole("button", { name: "Set password" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(calls.find((c) => c.url === "/api/users/2/password")?.body).toEqual({ password: "a new long password" });
  });

  it("escape closes a dialog", async () => {
    server();
    const { user } = await openUsers();
    await user.click(screen.getByRole("button", { name: "New user" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("userChanges", () => {
  it("contains only changed fields", () => {
    expect(userChanges(helperUser, { displayName: "Helper", disabled: false, permissions: ["search.use"] })).toEqual(
      {},
    );
    expect(userChanges(helperUser, { displayName: " Helper ", disabled: true, permissions: ["search.use"] })).toEqual({
      disabled: true,
    });
    expect(userChanges(helperUser, { displayName: "Hilde", disabled: false, permissions: [] })).toEqual({
      display_name: "Hilde",
      permissions: [],
    });
  });
});
