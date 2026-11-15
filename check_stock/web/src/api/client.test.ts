import { describe, expect, it, vi } from "vitest";
import { fakeServer, unauthenticated } from "../test/server";
import { api, ApiError, CSRF_HEADER, download, downloadFileName, setUnauthenticatedHandler } from "./client";

describe("api client", () => {
  it("sends the CSRF header only on requests that change something", async () => {
    const { calls } = fakeServer({
      "GET /api/users": { body: [] },
      "POST /api/users": { status: 201, body: {} },
      "PUT /api/settings": { body: {} },
      "PATCH /api/users/1": { body: {} },
      "DELETE /api/users/1": { status: 204 },
    });
    await api.get("/api/users");
    await api.post("/api/users", { username: "a" });
    await api.put("/api/settings", { a: 1 });
    await api.patch("/api/users/1", { disabled: true });
    await api.delete("/api/users/1");
    expect(calls.map((c) => [c.method, c.headers[CSRF_HEADER]])).toEqual([
      ["GET", undefined],
      ["POST", "1"],
      ["PUT", "1"],
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

  it("downloads files with the CSRF header and a safe file name", async () => {
    const fetchMock = vi.fn(
      async () =>
        new Response("a,b\n1,2\n", {
          status: 200,
          headers: { "Content-Type": "text/csv", "Content-Disposition": 'attachment; filename="buy_offer.csv"' },
        }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const file = await download("/api/export", { x: 1 }, "fallback.csv");
    expect(file.fileName).toBe("buy_offer.csv");
    expect(await file.blob.text()).toBe("a,b\n1,2\n");
    const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>)[CSRF_HEADER]).toBe("1");
    expect(init.body).toBe(JSON.stringify({ x: 1 }));
  });

  it("asks the browser to finish keepalive requests after the page closed", async () => {
    const { calls } = fakeServer({ "PUT /api/settings": { body: {} } });
    await api.put("/api/settings", { a: 1 }, { keepalive: true });
    await api.put("/api/settings", { a: 2 });
    expect(calls.map((c) => c.keepalive)).toEqual([true, false]);
  });

  it("refuses odd download file names", () => {
    expect(downloadFileName('attachment; filename="offer-1.csv"', "f.csv")).toBe("offer-1.csv");
    for (const header of [
      null,
      "attachment",
      'attachment; filename="../../etc/passwd"',
      'attachment; filename="a b.csv"',
      'attachment; filename=".hidden"',
      'attachment; filename=""',
      `attachment; filename="${"a".repeat(101)}"`,
    ]) {
      expect(downloadFileName(header, "f.csv"), String(header)).toBe("f.csv");
    }
  });

  it("turns a failed download into ApiError", async () => {
    fakeServer({
      "POST /api/export": {
        status: 413,
        body: { error: { code: "too_large", message: "The card list is too large" } },
      },
    });
    await expect(download("/api/export", {}, "f.csv")).rejects.toMatchObject({ status: 413, code: "too_large" });
  });
});
