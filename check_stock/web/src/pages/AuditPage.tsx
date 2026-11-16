// The audit log: who did what and when, newest first.

import { useAuditLog, usePermissions } from "../api/queries";
import type { JsonValue } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatTime } from "./UsersPage";

export const ACTION_LABELS: Record<string, string> = {
  "auth.login": "Logged in",
  "auth.login_failed": "Login failed",
  "auth.account_locked": "Account locked",
  "auth.logout": "Logged out",
  "auth.password_changed": "Changed own password",
  "user.created": "User created",
  "user.updated": "User changed",
  "user.password_reset": "Password set",
  "user.deleted": "User deleted",
  "import.desktop": "Desktop data imported",
  "import.refresh": "Desktop data refreshed",
  "settings.changed": "Settings changed",
  "inventory.imported": "Inventory imported",
  "inventory.import_forced": "Blocked inventory import forced",
  "inventory.restored": "Inventory restored from a backup",
  "inventory.discarded": "Cards discarded",
  "bins.list_created": "Bin move list made",
  "bins.list_discarded": "Bin move list discarded",
  "picking.created": "Picking list made",
  "lot_cost.changed": "Lot cost changed",
  "bulk_lot.finished": "Bulk lot recorded",
  "bulk_lot.price_changed": "Bulk lot price corrected",
  "bulk_lot.marked_removed": "Bulk lot removal marked done",
  "bulk_lot.undone": "Bulk lot undone",
};

/** Why a login failed, in words. */
export const LOGIN_FAILURES: Record<string, string> = {
  wrong_password: "wrong password",
  unknown_user: "unknown user",
  locked: "account locked",
  disabled: "account disabled",
};

/**
 * Details as short text: `key: value` pairs, lists joined with commas.
 * Permission keys become their labels (`labels`), login failure reasons words.
 */
export function formatDetails(details: JsonValue, labels: Map<string, string> = new Map()): string {
  const show = (value: JsonValue | undefined, key?: string): string => {
    if (value === null || value === undefined) return "—";
    if (Array.isArray(value)) return value.length ? value.map((v) => show(v, key)).join(", ") : "none";
    if (typeof value === "object") {
      return Object.entries(value)
        .map(([k, v]) => `${k.replaceAll("_", " ")}: ${show(v, k)}`)
        .join("; ");
    }
    const text = String(value);
    if (key === "reason") return LOGIN_FAILURES[text] ?? text;
    return labels.get(text) ?? text;
  };
  if (
    details === null ||
    (typeof details === "object" && !Array.isArray(details) && Object.keys(details).length === 0)
  ) {
    return "";
  }
  return show(details);
}

export function AuditPage() {
  const log = useAuditLog();
  const { data: catalog = [] } = usePermissions();
  const labels = new Map(catalog.map((p) => [p.key, p.label]));
  const entries = log.data?.pages.flatMap((page) => page.entries) ?? [];

  return (
    <section className="page">
      <h1>Audit log</h1>
      <ErrorMessage error={log.error} />
      {log.isPending ? (
        <p className="loading">Loading…</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>When</th>
                <th>Who</th>
                <th>What</th>
                <th>About</th>
                <th>Details</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr key={entry.id}>
                  <td className="nowrap">{formatTime(entry.at)}</td>
                  <td>{entry.actor}</td>
                  <td>{ACTION_LABELS[entry.action] ?? entry.action}</td>
                  <td>{entry.target ?? ""}</td>
                  <td className="details">{formatDetails(entry.details, labels)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {log.hasNextPage && (
        <button type="button" className="button" disabled={log.isFetchingNextPage} onClick={() => log.fetchNextPage()}>
          {log.isFetchingNextPage ? "Loading…" : "Load older entries"}
        </button>
      )}
    </section>
  );
}
