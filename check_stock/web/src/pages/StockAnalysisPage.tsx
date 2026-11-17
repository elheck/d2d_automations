// Stock analysis: what is in stock, how fast it sells, how long it has been
// sitting and how each purchase lot is doing. Money figures (values,
// revenue, costs, margins) only appear for users allowed to see them — the
// server leaves them out otherwise. Users who may edit lot costs click a
// lot's cost to enter or correct it.

import { useState } from "react";
import { can, useMe, useSetLotCost, useStockAnalysis } from "../api/queries";
import type { LotRow, NamedCount, StockAnalysisView } from "../api/types";
import { DataAsOf } from "../components/DataAsOf";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount, formatEuro } from "../format";

export type LotColumn = "lot" | "listings" | "copies" | "value" | "sold" | "revenue" | "cost" | "margin";

const LOT_COLUMNS: { key: LotColumn; label: string; money?: boolean }[] = [
  { key: "lot", label: "Lot" },
  { key: "listings", label: "In stock" },
  { key: "copies", label: "Copies" },
  { key: "value", label: "Stock value", money: true },
  { key: "sold", label: "Sold" },
  { key: "revenue", label: "Revenue", money: true },
  { key: "cost", label: "Cost", money: true },
  { key: "margin", label: "Margin", money: true },
];

/** `L12` → 12, so lots sort by number (L2 before L10). */
function lotNumber(lot: string): number {
  const n = Number(lot.replace(/\D/g, ""));
  return Number.isFinite(n) ? n : 0;
}

/** Lots sorted by `column`; missing values (no cost, no margin) always last. */
export function sortLots(rows: LotRow[], column: LotColumn, desc: boolean): LotRow[] {
  const value = (r: LotRow): number | null => {
    switch (column) {
      case "lot":
        return lotNumber(r.lot);
      case "listings":
        return r.in_stock_listings;
      case "copies":
        return r.in_stock_copies;
      case "value":
        return r.in_stock_value;
      case "sold":
        return r.sold_copies;
      case "revenue":
        return r.sold_revenue;
      case "cost":
        return r.cost;
      case "margin":
        return r.margin;
    }
  };
  return [...rows].sort((a, b) => {
    const va = value(a);
    const vb = value(b);
    if (va === null && vb === null) return a.lot.localeCompare(b.lot);
    if (va === null) return 1;
    if (vb === null) return -1;
    const order = va - vb || a.lot.localeCompare(b.lot);
    return desc ? -order : order;
  });
}

/** `0.35` → `+35%`. */
export function formatMargin(margin: number | null): string {
  if (margin === null) return "—";
  return `${margin >= 0 ? "+" : "−"}${Math.abs(Math.round(margin * 100))}%`;
}

export function StockAnalysisPage() {
  const analysis = useStockAnalysis();
  return (
    <section className="page page--wide">
      <h1>Stock analysis</h1>
      <DataAsOf />
      <ErrorMessage error={analysis.error} />
      {analysis.isPending && <p className="loading">Loading…</p>}
      {analysis.data && <Analysis data={analysis.data} />}
    </section>
  );
}

function Kpi({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className={tone ? `kpi kpi--${tone}` : "kpi"}>
      <span className="kpi__label">{label}</span>
      <span className="kpi__value">{value}</span>
    </div>
  );
}

function CountList({ label, items }: { label: string; items: NamedCount[] }) {
  if (items.length === 0) return null;
  return (
    <p>
      <strong>{label}:</strong> {items.map((i) => `${i.name.trim() || "unknown"} ${formatCount(i.count)}`).join(" · ")}
    </p>
  );
}

function Analysis({ data }: { data: StockAnalysisView }) {
  const v = data.velocity;
  const agingTotal = data.aging.reduce((sum, b) => sum + (b.value ?? 0), 0);
  return (
    <>
      <div className="kpis">
        <Kpi label="Listings" value={formatCount(data.total_articles)} />
        <Kpi label="In stock" value={formatCount(data.in_stock_articles)} />
        <Kpi label="Copies" value={formatCount(data.total_copies)} />
        {data.total_value !== null && <Kpi label="Stock value" value={formatEuro(data.total_value)} tone="good" />}
        <Kpi label="Foils" value={formatCount(data.foil_count)} />
        <Kpi label="Signed" value={formatCount(data.signed_count)} />
        {data.first_synced_date && <Kpi label="In stock since" value={data.first_synced_date} />}
      </div>

      <h2>Sales velocity</h2>
      {v ? (
        <>
          <p className="muted">Tracked over {v.period_days} days.</p>
          <div className="kpis">
            <Kpi label="Sold" value={`×${formatCount(v.sold_copies)}`} />
            <Kpi label="Copies / week" value={v.copies_per_week.toFixed(1)} />
            <Kpi label="Last 7 days" value={v.last7_copies === null ? "—" : `×${formatCount(v.last7_copies)}`} />
            <Kpi label="Last 30 days" value={v.last30_copies === null ? "—" : `×${formatCount(v.last30_copies)}`} />
            {v.sold_revenue !== null && <Kpi label="Revenue" value={formatEuro(v.sold_revenue)} tone="good" />}
            {v.revenue_per_week !== null && <Kpi label="Revenue / week" value={formatEuro(v.revenue_per_week)} />}
          </div>
          {v.bulk_lots > 0 && (
            <p className="muted small">
              Includes {formatCount(v.bulk_copies)} cards sold in {v.bulk_lots} bulk lot{v.bulk_lots === 1 ? "" : "s"}
              {v.bulk_revenue !== null && ` for ${formatEuro(v.bulk_revenue)}`}.
            </p>
          )}
        </>
      ) : (
        <p className="muted">Not enough history yet (needs inventory imports on two days).</p>
      )}

      {data.aging.some((b) => b.copies > 0) && (
        <>
          <h2>Dead-stock aging</h2>
          <p className="muted">How long the cards in stock have been listed.</p>
          <div className="table-wrap analysis-narrow">
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">Listed for</th>
                  <th scope="col" className="num">
                    Listings
                  </th>
                  <th scope="col" className="num">
                    Copies
                  </th>
                  {data.money && (
                    <th scope="col" className="num">
                      Capital
                    </th>
                  )}
                </tr>
              </thead>
              <tbody>
                {data.aging.map((b) => (
                  <tr key={b.label}>
                    <th scope="row">{b.label}</th>
                    <td className="num">{formatCount(b.listings)}</td>
                    <td className="num">×{formatCount(b.copies)}</td>
                    {data.money && <td className="num">{formatEuro(b.value)}</td>}
                  </tr>
                ))}
                {data.money && (
                  <tr className="offer__total">
                    <th scope="row">Total</th>
                    <td />
                    <td />
                    <td className="num">{formatEuro(agingTotal)}</td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </>
      )}

      <div className="analysis-columns">
        <section>
          <h2>Most copies</h2>
          <ol className="top-list">
            {data.top_by_quantity.map((t, i) => (
              <li key={`${i}-${t.name}`}>
                <span>{t.name}</span>
                <span className="num">×{formatCount(t.count)}</span>
              </li>
            ))}
          </ol>
        </section>
        <section>
          <h2>Most expensive</h2>
          <ol className="top-list">
            {data.top_by_price.map((t, i) => (
              <li key={`${i}-${t.name}`}>
                <span>{t.name}</span>
                <span className="num">{formatEuro(t.price)}</span>
              </li>
            ))}
          </ol>
        </section>
      </div>

      {data.longest_unsold.length > 0 && (
        <>
          <h2>Longest unsold</h2>
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">Card</th>
                  <th scope="col">Since</th>
                  <th scope="col" className="num">
                    Qty
                  </th>
                  <th scope="col" className="num">
                    Price
                  </th>
                  <th scope="col">Location</th>
                </tr>
              </thead>
              <tbody>
                {data.longest_unsold.map((u, i) => (
                  <tr key={`${i}-${u.name}`}>
                    <td>{u.name}</td>
                    <td className="nowrap">{u.date}</td>
                    <td className="num">×{u.quantity}</td>
                    <td className="num">{formatEuro(u.price)}</td>
                    <td className="nowrap">{u.location || "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}

      <h2>Breakdown</h2>
      {data.oldest_listed && (
        <p>
          <strong>Oldest listing:</strong> {data.oldest_listed.name} ({data.oldest_listed.date})
          {data.newest_listed && (
            <>
              {" · "}
              <strong>Newest:</strong> {data.newest_listed.name} ({data.newest_listed.date})
            </>
          )}
        </p>
      )}
      <CountList label="Languages" items={data.languages} />
      <CountList label="Conditions" items={data.conditions} />
      <CountList label="Rarities" items={data.rarities} />

      {data.lots.length > 0 && <Lots data={data} />}
    </>
  );
}

/** `"12,50"` / `"12.5"` → 12.5; empty → `null` (no cost); otherwise `undefined`. */
export function parseCost(text: string): number | null | undefined {
  const t = text.trim().replace(",", ".");
  if (t === "") return null;
  if (!/^\d{1,7}(\.\d{1,2})?$/.test(t)) return undefined;
  const value = Number(t);
  return value <= 1_000_000 ? value : undefined;
}

/** The cost cell: a button that opens a small editor. */
function CostCell({ lot }: { lot: LotRow }) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const save = useSetLotCost();
  const parsed = parseCost(text);
  if (!editing) {
    return (
      <td className="num">
        <button
          type="button"
          className="link-button"
          onClick={() => {
            setText(lot.cost === null ? "" : lot.cost.toFixed(2));
            save.reset();
            setEditing(true);
          }}
          aria-label={`Edit the cost of ${lot.lot}`}
        >
          {formatEuro(lot.cost)}
        </button>
      </td>
    );
  }
  return (
    <td className="num">
      <form
        className="cost-editor"
        onSubmit={(e) => {
          e.preventDefault();
          if (parsed === undefined) return;
          save.mutate({ lot: lot.lot, cost: parsed, expected: lot.cost }, { onSuccess: () => setEditing(false) });
        }}
      >
        <input
          className="number-field__input"
          inputMode="decimal"
          value={text}
          aria-label={`Cost of ${lot.lot} in euros (empty: none)`}
          aria-invalid={parsed === undefined}
          onChange={(e) => setText(e.target.value)}
          autoFocus
        />
        <button type="submit" className="button button--small" disabled={parsed === undefined || save.isPending}>
          Save
        </button>
        <button type="button" className="button button--ghost button--small" onClick={() => setEditing(false)}>
          Cancel
        </button>
        <ErrorMessage error={save.error} />
      </form>
    </td>
  );
}

function Lots({ data }: { data: StockAnalysisView }) {
  const { data: me } = useMe();
  const mayEdit = data.money && can(me, "lot_costs.edit");
  const [order, setOrder] = useState<{ column: LotColumn; desc: boolean }>({ column: "lot", desc: false });
  const columns = LOT_COLUMNS.filter((c) => data.money || !c.money);
  const rows = sortLots(data.lots, order.column, order.desc);
  const totals = data.lot_totals;
  return (
    <>
      <h2>Purchase lots</h2>
      <p className="muted">
        {data.money
          ? mayEdit
            ? "Click a lot's cost to enter or correct it. The margin counts only money already earned."
            : "The margin counts only money already earned."
          : "Values, revenue, costs and margins need the permission “View revenue and costs”."}
      </p>
      <div className="table-wrap">
        <table className="table" aria-label="Purchase lots">
          <thead>
            <tr>
              {columns.map((c) => {
                const active = c.key === order.column;
                return (
                  <th
                    key={c.key}
                    scope="col"
                    className={c.key === "lot" ? undefined : "num"}
                    aria-sort={active ? (order.desc ? "descending" : "ascending") : undefined}
                  >
                    <button
                      type="button"
                      className="sort-button"
                      onClick={() =>
                        setOrder((current) =>
                          current.column === c.key
                            ? { column: c.key, desc: !current.desc }
                            : { column: c.key, desc: c.key !== "lot" },
                        )
                      }
                    >
                      {c.label}
                      <span aria-hidden="true">{active ? (order.desc ? " ⏷" : " ⏶") : ""}</span>
                    </button>
                  </th>
                );
              })}
              {data.money && <th scope="col">Payback</th>}
            </tr>
          </thead>
          <tbody>
            {rows.map((l) => {
              const bulkNote =
                l.bulk_copies > 0
                  ? `Includes ${l.bulk_copies} cards sold in bulk lots${l.bulk_revenue !== null ? ` for ${formatEuro(l.bulk_revenue)}` : ""}`
                  : undefined;
              return (
                <tr key={l.lot}>
                  <th scope="row">{l.lot}</th>
                  <td className="num">{formatCount(l.in_stock_listings)}</td>
                  <td className="num">×{formatCount(l.in_stock_copies)}</td>
                  {data.money && <td className="num">{formatEuro(l.in_stock_value)}</td>}
                  <td className="num" title={bulkNote}>
                    ×{formatCount(l.sold_copies)}
                  </td>
                  {data.money && (
                    <>
                      <td className="num" title={bulkNote}>
                        {formatEuro(l.sold_revenue)}
                      </td>
                      {mayEdit ? <CostCell lot={l} /> : <td className="num">{formatEuro(l.cost)}</td>}
                      <td
                        className={
                          l.margin === null ? "num muted" : l.margin >= 0 ? "num change--up" : "num change--down"
                        }
                      >
                        {formatMargin(l.margin)}
                      </td>
                      <td className="nowrap">
                        {l.recouped === true ? (
                          <span className="change--up">✓ recouped</span>
                        ) : l.recouped === false ? (
                          <span className="change--down">{formatEuro(l.cost_to_recoup)} to go</span>
                        ) : (
                          <span className="muted">—</span>
                        )}
                      </td>
                    </>
                  )}
                </tr>
              );
            })}
            <tr className="offer__total">
              <th scope="row">Total</th>
              <td />
              <td />
              {totals && <td className="num">{formatEuro(totals.in_stock_value)}</td>}
              <td className="num">×{formatCount(data.lots_sold_copies)}</td>
              {totals && (
                <>
                  <td className="num">{formatEuro(totals.sold_revenue)}</td>
                  <td className="num">{formatEuro(totals.cost)}</td>
                  <td className="num" title="Across the lots that have a recorded cost">
                    {formatMargin(totals.margin)}
                  </td>
                  <td />
                </>
              )}
            </tr>
          </tbody>
        </table>
      </div>
      {data.bulk_lots > 0 && (
        <p className="muted small">
          Sold and revenue include {formatCount(data.bulk_copies)} cards sold in {data.bulk_lots} bulk lot
          {data.bulk_lots === 1 ? "" : "s"}
          {data.bulk_revenue !== null && ` for ${formatEuro(data.bulk_revenue)}`} (each card earns its share of the lot
          price).
        </p>
      )}
    </>
  );
}
