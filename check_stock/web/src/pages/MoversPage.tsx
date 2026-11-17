// Price movers: cards in stock whose market price moved over 7 or 30 days —
// spikes worth selling into (raise the price before the market corrects) and
// falling cards worth selling off. The market data is loaded on the server
// once a day for everyone; opening this page starts that load if needed.

import { useState } from "react";
import { useMarketStatus, useMovers, type MoversParams } from "../api/queries";
import type { MarketStatusView, MoverDirectionKey, MoverSortKey, PriceColumn } from "../api/types";
import { DataAsOf } from "../components/DataAsOf";
import { ErrorMessage } from "../components/ErrorMessage";
import { MarketPanel } from "../components/MarketPanel";
import { formatEuro } from "../format";
import { useDebounced } from "../useDebounced";
import { parseDecimal } from "./buyHelper";
import { isCardmarketUrl, PRICE_COLUMNS } from "./preconEv";

const COLUMNS: { label: string; sort: MoverSortKey; numeric?: boolean }[] = [
  { label: "Card", sort: "name" },
  { label: "Set", sort: "set" },
  { label: "Qty", sort: "quantity", numeric: true },
  { label: "Age", sort: "age", numeric: true },
  { label: "Listed", sort: "listed", numeric: true },
  { label: "Market", sort: "current", numeric: true },
  { label: "Δ7d", sort: "change7", numeric: true },
  { label: "Δ30d", sort: "change30", numeric: true },
  { label: "Location", sort: "location" },
];

const DIRECTIONS: { value: MoverDirectionKey; label: string }[] = [
  { value: "all", label: "All" },
  { value: "risers", label: "Risers" },
  { value: "fallers", label: "Fallers" },
];

/** `12.3456` → `+12.3%`; nothing → `—`. */
export function formatChange(pct: number | null): string {
  if (pct === null) return "—";
  const sign = pct > 0 ? "+" : pct < 0 ? "−" : "±";
  return `${sign}${Math.abs(pct).toFixed(1)}%`;
}

export function changeClass(pct: number | null): string {
  if (pct === null || pct === 0) return "num";
  return pct > 0 ? "num change--up" : "num change--down";
}

/** Same column: flip; another: descending (text columns ascending). */
export function nextMoverSort(current: { sort: MoverSortKey; desc: boolean }, column: MoverSortKey) {
  if (current.sort === column) return { sort: column, desc: !current.desc };
  return { sort: column, desc: !["name", "set", "location"].includes(column) };
}

/** A whole number of days from 0 to 3650, or `null`. */
export function parseDays(text: string): number | null {
  if (!/^\s*\d{1,4}\s*$/.test(text)) return null;
  const value = Number(text);
  return value <= 3650 ? value : null;
}

export function MoversPage() {
  const status = useMarketStatus("/api/movers");
  return (
    <section className="page page--wide">
      <h1>Price movers</h1>
      <DataAsOf />
      <ErrorMessage error={status.error} />
      {status.data && <Movers status={status.data} />}
    </section>
  );
}

function Movers({ status }: { status: MarketStatusView }) {
  const [field, setField] = useState<PriceColumn>("trend");
  const [minPriceText, setMinPriceText] = useState("0.30");
  const [minAgeText, setMinAgeText] = useState("0");
  const [direction, setDirection] = useState<MoverDirectionKey>("all");
  const [order, setOrder] = useState<{ sort: MoverSortKey; desc: boolean }>({ sort: "change7", desc: true });
  const minPrice = parseDecimal(minPriceText);
  const minAge = parseDays(minAgeText);
  const priceOk = minPrice !== null && minPrice <= 1000;
  const params: MoversParams | null =
    priceOk && minAge !== null ? { field, minPrice, minAge, direction, ...order } : null;
  const settled = useDebounced(params, 250);
  const version = status.loaded?.version ?? null;
  const movers = useMovers(settled, version);

  if (!status.configured) return <MarketPanel base="/api/movers" status={status} />;

  return (
    <>
      <MarketPanel base="/api/movers" status={status} />

      <div className="panel precon-controls">
        <label className="inline-field">
          <span>Price field</span>
          <select value={field} onChange={(e) => setField(e.target.value as PriceColumn)}>
            {PRICE_COLUMNS.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <span className="inline-field">
          <label htmlFor="movers-min-price">Min price (€)</label>
          <input
            id="movers-min-price"
            className="number-field__input"
            value={minPriceText}
            inputMode="decimal"
            maxLength={8}
            aria-invalid={!priceOk}
            onChange={(e) => setMinPriceText(e.target.value)}
          />
        </span>
        <span className="inline-field">
          <label htmlFor="movers-min-age">Min age (days)</label>
          <input
            id="movers-min-age"
            className="number-field__input"
            value={minAgeText}
            inputMode="numeric"
            maxLength={4}
            aria-invalid={minAge === null}
            onChange={(e) => setMinAgeText(e.target.value)}
          />
        </span>
        <span className="segmented" role="group" aria-label="Direction">
          {DIRECTIONS.map((d) => (
            <button
              key={d.value}
              type="button"
              className={direction === d.value ? "segmented__button segmented__button--on" : "segmented__button"}
              aria-pressed={direction === d.value}
              onClick={() => setDirection(d.value)}
            >
              {d.label}
            </button>
          ))}
        </span>
        {!priceOk && (
          <p className="error" role="alert">
            Enter a minimum price from 0 to 1000.
          </p>
        )}
        {minAge === null && (
          <p className="error" role="alert">
            Enter a whole number of days from 0 to 3650.
          </p>
        )}
      </div>

      <ErrorMessage error={movers.error} />
      {movers.data && (
        <>
          <p className="muted">
            {movers.data.total > movers.data.rows.length
              ? `Showing the first ${movers.data.rows.length} of ${movers.data.total} movers`
              : `${movers.data.total} movers`}
            . Old stock that is also losing value is the first to sell off.
          </p>
          <div className={movers.isFetching ? "table-wrap updating" : "table-wrap"}>
            <table className="table">
              <thead>
                <tr>
                  {COLUMNS.map((column) => {
                    const active = column.sort === order.sort;
                    return (
                      <th
                        key={column.label}
                        scope="col"
                        className={column.numeric ? "num" : undefined}
                        aria-sort={active ? (order.desc ? "descending" : "ascending") : undefined}
                      >
                        <button
                          type="button"
                          className="sort-button"
                          onClick={() => setOrder((current) => nextMoverSort(current, column.sort))}
                        >
                          {column.label}
                          <span aria-hidden="true">{active ? (order.desc ? " ⏷" : " ⏶") : ""}</span>
                        </button>
                      </th>
                    );
                  })}
                </tr>
              </thead>
              <tbody>
                {movers.data.rows.map((row, index) => (
                  <tr key={`${index}-${row.name}-${row.set_code}-${row.condition}-${row.location}`}>
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
                    <td className="num">{row.quantity}</td>
                    <td className="num">{row.age_days} d</td>
                    <td className="num">{formatEuro(row.listed)}</td>
                    <td className="num">{formatEuro(row.market)}</td>
                    <td className={changeClass(row.pct_7d)}>{formatChange(row.pct_7d)}</td>
                    <td className={changeClass(row.pct_30d)}>{formatChange(row.pct_30d)}</td>
                    <td className="nowrap">{row.location}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </>
  );
}
