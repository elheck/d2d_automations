// Restock recommendations: sold-out cards ranked by how fast they sold, so
// the fastest sellers worth buying again are on top. Read-only; the list can
// be downloaded as a buy-list CSV. Money figures (realized revenue) only
// appear for users allowed to see them — the server leaves them out
// otherwise.

import { useState } from "react";
import { can, useExportRestock, useMe, useRestock, type RestockParams } from "../api/queries";
import type { RestockSortKey } from "../api/types";
import { DataAsOf } from "../components/DataAsOf";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount, formatEuro } from "../format";
import { useDebounced } from "../useDebounced";
import { saveDownload } from "./buyHelper";
import { isCardmarketUrl } from "./preconEv";

const COLUMNS: { label: string; sort?: RestockSortKey; numeric?: boolean; money?: boolean }[] = [
  { label: "Card", sort: "name" },
  { label: "Set" },
  { label: "Lot", sort: "lot" },
  { label: "Sold", sort: "sold_copies", numeric: true },
  { label: "Copies/wk", sort: "velocity", numeric: true },
  { label: "Days", sort: "days", numeric: true },
  { label: "Last price", sort: "price", numeric: true },
  { label: "Revenue", sort: "revenue", numeric: true, money: true },
  { label: "Sold out" },
];

/** Same column: flip; another: descending, except names (ascending). */
export function nextRestockSort(current: { sort: RestockSortKey; desc: boolean }, column: RestockSortKey) {
  if (current.sort === column) return { sort: column, desc: !current.desc };
  return { sort: column, desc: column !== "name" && column !== "lot" };
}

/** A whole number of copies from 1 to 1000, or `null`. */
export function parseMinCopies(text: string): number | null {
  if (!/^\s*\d{1,4}\s*$/.test(text)) return null;
  const value = Number(text);
  return value >= 1 && value <= 1000 ? value : null;
}

export function RestockPage() {
  const { data: me } = useMe();
  const money = can(me, "financials.view");
  const [minText, setMinText] = useState("2");
  const [order, setOrder] = useState<{ sort: RestockSortKey; desc: boolean }>({ sort: "velocity", desc: true });
  const minCopies = useDebounced(parseMinCopies(minText), 300);
  const params: RestockParams | null = minCopies === null ? null : { minCopies, ...order };
  const restock = useRestock(params);
  const exporting = useExportRestock();
  const columns = COLUMNS.filter((c) => money || !c.money);

  return (
    <section className="page page--wide">
      <h1>Restock</h1>
      <DataAsOf />
      <div className="panel precon-controls">
        <span className="inline-field">
          <label htmlFor="restock-min">Min copies sold</label>
          <input
            id="restock-min"
            className="number-field__input"
            value={minText}
            inputMode="numeric"
            maxLength={4}
            aria-invalid={parseMinCopies(minText) === null}
            onChange={(e) => setMinText(e.target.value)}
          />
        </span>
        <button
          type="button"
          className="button"
          disabled={!params || !restock.data?.total || exporting.isPending}
          onClick={() => params && exporting.mutate(params, { onSuccess: saveDownload })}
        >
          {exporting.isPending ? "Preparing…" : "Download buy list"}
        </button>
        <p className="muted">
          Sold-out cards ranked by how fast they sold: what to buy again. Raise the minimum to hide one-off sales.
        </p>
        {parseMinCopies(minText) === null && (
          <p className="error" role="alert">
            Enter a whole number from 1 to 1000.
          </p>
        )}
      </div>
      <ErrorMessage error={restock.error ?? exporting.error} />
      {restock.data && (
        <>
          <div className="kpis">
            <div className="kpi">
              <span className="kpi__label">Candidates</span>
              <span className="kpi__value">{formatCount(restock.data.total)}</span>
            </div>
            <div className="kpi">
              <span className="kpi__label">Copies sold</span>
              <span className="kpi__value">{formatCount(restock.data.copies)}</span>
            </div>
            {restock.data.revenue !== null && (
              <div className="kpi kpi--good">
                <span className="kpi__label">Realized revenue</span>
                <span className="kpi__value">{formatEuro(restock.data.revenue)}</span>
              </div>
            )}
          </div>
          <p className="muted">
            {restock.data.total > restock.data.rows.length
              ? `Showing the first ${restock.data.rows.length} of ${formatCount(restock.data.total)}; the buy list has all of them.`
              : `${formatCount(restock.data.total)} cards`}
          </p>
          {restock.data.total === 0 ? (
            <p className="muted">No sold-out cards with at least this many copies sold.</p>
          ) : (
            <div className={restock.isFetching ? "table-wrap updating" : "table-wrap"}>
              <table className="table">
                <thead>
                  <tr>
                    {columns.map((column) => {
                      const sortKey = column.sort;
                      const active = sortKey === order.sort;
                      return (
                        <th
                          key={column.label}
                          scope="col"
                          className={column.numeric ? "num" : undefined}
                          aria-sort={active ? (order.desc ? "descending" : "ascending") : undefined}
                        >
                          {sortKey ? (
                            <button
                              type="button"
                              className="sort-button"
                              onClick={() => setOrder((current) => nextRestockSort(current, sortKey))}
                            >
                              {column.label}
                              <span aria-hidden="true">{active ? (order.desc ? " ⏷" : " ⏶") : ""}</span>
                            </button>
                          ) : (
                            column.label
                          )}
                        </th>
                      );
                    })}
                  </tr>
                </thead>
                <tbody>
                  {restock.data.rows.map((row, index) => (
                    <tr key={`${index}-${row.name}-${row.set_code}-${row.condition}`}>
                      <td>
                        {isCardmarketUrl(row.cardmarket_url) ? (
                          <a href={row.cardmarket_url} target="_blank" rel="noopener noreferrer">
                            {row.name}
                          </a>
                        ) : (
                          row.name
                        )}
                        {row.is_foil && (
                          <span className="foil" title="Foil">
                            {" "}
                            ✦
                          </span>
                        )}
                      </td>
                      <td className="nowrap">
                        {row.set_code} {row.condition}
                      </td>
                      <td>{row.lot ?? "—"}</td>
                      <td className="num">×{row.sold_copies}</td>
                      <td className="num">{row.copies_per_week.toFixed(2)}</td>
                      <td className="num">{row.days_to_sell_out}</td>
                      <td className="num">{formatEuro(row.last_price)}</td>
                      {money && <td className="num">{formatEuro(row.realized_revenue)}</td>}
                      <td className="nowrap">{row.sold_out_date}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </section>
  );
}
