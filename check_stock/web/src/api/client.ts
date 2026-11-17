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

type Method = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

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

/** Extra fetch options. */
export interface RequestOptions {
  /** Let the browser finish the request even if the page is closing (small bodies only). */
  keepalive?: boolean;
}

/** Sends a request; resolves with the response if it succeeded, else throws {@link ApiError}. */
async function send(
  method: Method,
  path: string,
  body: unknown,
  accept: string,
  options: RequestOptions = {},
): Promise<Response> {
  const headers: Record<string, string> = { Accept: accept };
  if (method !== "GET") headers[CSRF_HEADER] = "1";
  if (body !== undefined) headers["Content-Type"] = "application/json";

  let response: Response;
  try {
    response = await fetch(path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "same-origin",
      keepalive: options.keepalive ?? false,
    });
  } catch {
    throw new ApiError(0, "network", "The server cannot be reached.");
  }
  if (response.ok) return response;

  const data = parseJson(await response.text());
  const error = isErrorBody(data)
    ? new ApiError(response.status, data.error.code, data.error.message)
    : new ApiError(response.status, "http_error", `Request failed (${response.status}).`);
  if (error.status === 401 && error.code === "unauthenticated") onUnauthenticated();
  throw error;
}

function parseJson(text: string): unknown {
  if (!text) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

export async function request<T>(method: Method, path: string, body?: unknown, options?: RequestOptions): Promise<T> {
  const response = await send(method, path, body, "application/json", options);
  if (response.status === 204) return undefined as T;
  return parseJson(await response.text()) as T;
}

/** A file the server sent for download. */
export interface Download {
  blob: Blob;
  fileName: string;
}

/**
 * The file name from a `Content-Disposition` header: only letters, digits,
 * `.`, `_` and `-` (anything else falls back to `fallback`).
 */
export function downloadFileName(header: string | null, fallback: string): string {
  const name = header?.match(/filename="([^"]*)"/)?.[1] ?? "";
  return /^[A-Za-z0-9._-]{1,100}$/.test(name) && !name.startsWith(".") ? name : fallback;
}

/**
 * Fetches a file the server answers with: POST with `body`, or GET when
 * `body` is `undefined` (e.g. a report export with its query string).
 */
export async function download(path: string, body: unknown, fallbackName: string): Promise<Download> {
  const response = await send(body === undefined ? "GET" : "POST", path, body, "*/*");
  return {
    blob: await response.blob(),
    fileName: downloadFileName(response.headers.get("Content-Disposition"), fallbackName),
  };
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {}),
  put: <T>(path: string, body: unknown, options?: RequestOptions) => request<T>("PUT", path, body, options),
  patch: <T>(path: string, body: unknown) => request<T>("PATCH", path, body),
  delete: <T>(path: string) => request<T>("DELETE", path),
};
