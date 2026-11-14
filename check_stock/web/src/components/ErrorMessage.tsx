import { ApiError } from "../api/client";

/** Shows what went wrong, as the server put it. */
export function ErrorMessage({ error }: { error: unknown }) {
  if (!error) return null;
  const message =
    error instanceof ApiError ? error.message : error instanceof Error ? error.message : "Something went wrong.";
  return (
    <p className="error" role="alert">
      {message}
    </p>
  );
}
