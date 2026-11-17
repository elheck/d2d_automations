import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { catalog, helper } from "../test/fixtures";
import { renderApp } from "../test/renderApp";
import { fakeServer } from "../test/server";

function server(extra = {}) {
  return fakeServer({
    "GET /api/auth/me": { body: helper },
    "GET /api/permissions": { body: catalog },
    ...extra,
  });
}

describe("account page", () => {
  it("lists the user's permissions with their descriptions", async () => {
    server();
    renderApp("/account");
    expect(await screen.findByText("Search cards")).toBeInTheDocument();
    expect(screen.getByText(/Search the inventory and see prices/)).toBeInTheDocument();
    expect(screen.queryByText("Manage users")).not.toBeInTheDocument();
  });

  it("changes the password", async () => {
    const { calls } = server({ "POST /api/auth/password": { status: 204 } });
    const { user } = renderApp("/account");
    await user.type(await screen.findByLabelText("Current password"), "the old password");
    await user.type(screen.getByLabelText(/^New password/), "a brand new password");
    await user.type(screen.getByLabelText("Repeat new password"), "a brand new password");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Password changed");
    expect(calls.find((c) => c.url === "/api/auth/password")?.body).toEqual({
      current_password: "the old password",
      new_password: "a brand new password",
    });
  });

  it("catches mistakes before sending", async () => {
    const { calls } = server();
    const { user } = renderApp("/account");
    await user.type(await screen.findByLabelText("Current password"), "the old password");
    await user.type(screen.getByLabelText(/^New password/), "a brand new password");
    await user.type(screen.getByLabelText("Repeat new password"), "a different password");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(screen.getByRole("alert")).toHaveTextContent("differ");
    expect(calls.some((c) => c.method === "POST")).toBe(false);
  });

  it("shows a wrong current password", async () => {
    server({
      "POST /api/auth/password": {
        status: 400,
        body: { error: { code: "invalid_request", message: "The current password is wrong" } },
      },
    });
    const { user } = renderApp("/account");
    await user.type(await screen.findByLabelText("Current password"), "not the password");
    await user.type(screen.getByLabelText(/^New password/), "a brand new password");
    await user.type(screen.getByLabelText("Repeat new password"), "a brand new password");
    await user.click(screen.getByRole("button", { name: "Change password" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The current password is wrong");
  });
});
