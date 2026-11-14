import { Link } from "@tanstack/react-router";

export function NotFoundPage() {
  return (
    <section className="page">
      <h1>Page not found</h1>
      <p>
        <Link to="/">Back to the start page</Link>
      </p>
    </section>
  );
}
