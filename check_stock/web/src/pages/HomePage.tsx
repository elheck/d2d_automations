import { Link } from "@tanstack/react-router";
import { useDigest, useMe } from "../api/queries";
import type { DigestView } from "../api/types";
import { formatCount, formatEuro } from "../format";
import { visibleGroups } from "../navigation";

export function HomePage() {
  const { data: me } = useMe();
  return (
    <section className="page">
      <h1>Hello, {me?.display_name}</h1>
      <Digest />
      <p className="muted">
        The screens of the check_stock desktop app move here step by step. Until then the desktop app stays in use.
      </p>
      {visibleGroups(me).map((group) => (
        <section key={group.label} aria-label={group.label}>
          <h2>{group.label}</h2>
          <div className="tiles">
            {group.items.map((item) => (
              <Link key={item.to} to={item.to} className="tile">
                <span className="tile__title">{item.label}</span>
                <span className="tile__text">{item.description}</span>
              </Link>
            ))}
          </div>
        </section>
      ))}
      <h2>You</h2>
      <div className="tiles">
        <Link to="/account" className="tile">
          <span className="tile__title">Your account</span>
          <span className="tile__text">Your permissions and password.</span>
        </Link>
      </div>
    </section>
  );
}

/** Days between two `YYYY-MM-DD` dates (local calendar). */
export function daysSince(day: string, today: Date = new Date()): number | null {
  const [y, m, d] = day.split("-").map(Number);
  if (!y || !m || !d) return null;
  const then = new Date(y, m - 1, d);
  const now = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  return Math.round((now.getTime() - then.getTime()) / 86_400_000);
}

/** The "since your last visit" line, as on the desktop's welcome screen. */
export function digestParts(d: DigestView): string[] {
  const parts: string[] = [];
  if (d.since && d.sold_copies !== null) {
    const revenue = d.sold_revenue !== null ? ` (${formatEuro(d.sold_revenue)})` : "";
    parts.push(
      d.sold_copies > 0
        ? `${formatCount(d.sold_copies)} copies sold${revenue} since ${d.since}`
        : `no sales recorded since ${d.since}`,
    );
    if (d.new_listings) parts.push(`${formatCount(d.new_listings)} new listings`);
    if (d.bulk_lots) {
      const money = d.bulk_revenue !== null ? ` (${formatEuro(d.bulk_revenue)})` : "";
      parts.push(`${d.bulk_lots} bulk lot${d.bulk_lots === 1 ? "" : "s"}${money}`);
    }
  }
  if (d.restock_candidates) parts.push(`${formatCount(d.restock_candidates)} restock candidates`);
  return parts;
}

/** How fresh the inventory is; stale after a week (every report depends on it). */
export function freshness(lastImport: string | null, today: Date = new Date()): { text: string; stale: boolean } {
  if (!lastImport) return { text: "No inventory imported yet.", stale: false };
  const age = daysSince(lastImport, today);
  if (age === null) return { text: `Last inventory import ${lastImport}.`, stale: false };
  if (age <= 0) return { text: "Inventory imported today.", stale: false };
  if (age > 7) return { text: `Last inventory import ${age} days ago — import a fresh export.`, stale: true };
  return { text: `Last inventory import ${age} day${age === 1 ? "" : "s"} ago.`, stale: false };
}

function Digest() {
  const { data } = useDigest();
  if (!data) return null;
  const parts = digestParts(data);
  const fresh = freshness(data.last_import);
  return (
    <>
      {parts.length > 0 && <p className="digest">{parts.join(" · ")}</p>}
      <p className={fresh.stale ? "digest digest--stale" : "digest muted"}>{fresh.text}</p>
    </>
  );
}
