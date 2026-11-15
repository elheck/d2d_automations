// A fake check_stock server for tests: answers fetch() by "METHOD /path"
// and records every request (so tests can check bodies and headers).

import { vi } from "vitest";

export interface Call {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: unknown;
  keepalive: boolean;
}

export interface Reply {
  status?: number;
  body?: unknown;
}

export type Route = Reply | ((call: Call) => Reply);

export function fakeServer(routes: Record<string, Route>) {
  const calls: Call[] = [];
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    const method = init?.method ?? "GET";
    const headers = { ...(init?.headers as Record<string, string> | undefined) };
    const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
    const call = { method, url, headers, body, keepalive: init?.keepalive ?? false };
    calls.push(call);
    const route = routes[`${method} ${url}`] ?? routes[`${method} ${url.split("?")[0]}`];
    const reply: Reply = route
      ? typeof route === "function"
        ? route(call)
        : route
      : { status: 404, body: { error: { code: "not_found", message: "Not found" } } };
    const status = reply.status ?? 200;
    return new Response(status === 204 ? null : JSON.stringify(reply.body ?? {}), {
      status,
      headers: { "Content-Type": "application/json" },
    });
  });
  vi.stubGlobal("fetch", fetchMock);
  return { calls, fetchMock };
}

export const unauthenticated: Reply = {
  status: 401,
  body: { error: { code: "unauthenticated", message: "Please log in" } },
};
