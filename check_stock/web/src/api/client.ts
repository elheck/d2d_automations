// Talks to the check_stock server. Same origin only: the session cookie is
// sent automatically (HttpOnly, invisible to scripts), and every request that
// changes something carries the CSRF header the server requires.

import type { ApiErrorBody } from "./types";

export const CSRF_HEADER = "X-Check-Stock-CSRF";

/** An error answer from the server, with its machine-readable code. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

type Method = "GET" | "POST" | "PATCH" | "DELETE";

let onUnauthenticated: () => void = () => {};

/** Called whenever the server says the session is gone (401). */
export function setUnauthenticatedHandler(handler: () => void): void {
  onUnauthenticated = handler;
}

function isErrorBody(value: unknown): value is ApiErrorBody {
  if (typeof value !== "object" || value === null || !("error" in value)) return false;
  const error = (value as { error: unknown }).error;
  return (
    typeof error === "object" &&
    error !== null &&
    typeof (error as { code?: unknown }).code === "string" &&
    typeof (error as { message?: unknown }).message === "string"
  );
}

export async function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json" };
  if (method !== "GET") headers[CSRF_HEADER] = "1";
  if (body !== undefined) headers["Content-Type"] = "application/json";

  let response: Response;
  try {
    response = await fetch(path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "same-origin",
    });
  } catch {
    throw new ApiError(0, "network", "The server cannot be reached.");
  }

  if (response.status === 204) return undefined as T;
  const text = await response.text();
  let data: unknown = undefined;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = undefined;
    }
  }
  if (!response.ok) {
    const error = isErrorBody(data)
      ? new ApiError(response.status, data.error.code, data.error.message)
      : new ApiError(response.status, "http_error", `Request failed (${response.status}).`);
    if (error.status === 401 && error.code === "unauthenticated") onUnauthenticated();
    throw error;
  }
  return data as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  patch: <T>(path: string, body: unknown) => request<T>("PATCH", path, body),
  delete: <T>(path: string) => request<T>("DELETE", path),
};
