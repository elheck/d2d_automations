import { describe, expect, it, vi } from "vitest";
import { fakeServer, unauthenticated } from "../test/server";
import { api, ApiError, CSRF_HEADER, setUnauthenticatedHandler } from "./client";

describe("api client", () => {
  it("sends the CSRF header only on requests that change something", async () => {
    const { calls } = fakeServer({
      "GET /api/users": { body: [] },
      "POST /api/users": { status: 201, body: {} },
      "PATCH /api/users/1": { body: {} },
      "DELETE /api/users/1": { status: 204 },
    });
    await api.get("/api/users");
    await api.post("/api/users", { username: "a" });
    await api.patch("/api/users/1", { disabled: true });
    await api.delete("/api/users/1");
    expect(calls.map((c) => [c.method, c.headers[CSRF_HEADER]])).toEqual([
      ["GET", undefined],
      ["POST", "1"],
      ["PATCH", "1"],
      ["DELETE", "1"],
    ]);
    expect(calls[1]?.body).toEqual({ username: "a" });
    expect(calls[1]?.headers["Content-Type"]).toBe("application/json");
  });

  it("returns undefined for 204 and parsed JSON otherwise", async () => {
    fakeServer({ "POST /api/auth/logout": { status: 204 }, "GET /api/auth/me": { body: { username: "aron" } } });
    await expect(api.post("/api/auth/logout")).resolves.toBeUndefined();
    await expect(api.get("/api/auth/me")).resolves.toEqual({ username: "aron" });
  });

  it("turns error answers into ApiError with the server's code and message", async () => {
    fakeServer({
      "POST /api/users": {
        status: 409,
        body: { error: { code: "username_taken", message: "this user name is already taken" } },
      },
    });
    const error = await api.post("/api/users", {}).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 409, code: "username_taken", message: "this user name is already taken" });
  });

  it("copes with error answers that are not JSON", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("<html>bad gateway</html>", { status: 502 })),
    );
    await expect(api.get("/api/x")).rejects.toMatchObject({ status: 502, code: "http_error" });
  });

  it("reports an unreachable server", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Promise.reject(new TypeError("Failed to fetch"))),
    );
    await expect(api.get("/api/x")).rejects.toMatchObject({ status: 0, code: "network" });
  });

  it("tells the app when the session is gone", async () => {
    const handler = vi.fn();
    setUnauthenticatedHandler(handler);
    fakeServer({
      "GET /api/users": unauthenticated,
      "POST /api/auth/login": { status: 401, body: { error: { code: "invalid_credentials", message: "Wrong" } } },
    });
    await expect(api.get("/api/users")).rejects.toBeInstanceOf(ApiError);
    expect(handler).toHaveBeenCalledTimes(1);
    await expect(api.post("/api/auth/login", {})).rejects.toBeInstanceOf(ApiError);
    expect(handler).toHaveBeenCalledTimes(1);
  });
});
