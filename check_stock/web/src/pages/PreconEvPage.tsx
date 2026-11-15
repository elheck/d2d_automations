// Precon EV ranking: every Commander precon by the value of its singles
// against the sealed price. The data is loaded once on the server for
// everyone ("Load precons"); the settings here only change the calculation
// and are saved for the user.

import { Link } from "@tanstack/react-router";
import { useState } from "react";
import {
  savePreconSettingsOnExit,
  usePreconRanking,
  usePreconSettings,
  usePreconStatus,
  useSavePreconSettings,
  useStartPreconLoad,
} from "../api/queries";
import type { PreconSettings, PreconSettingsView, PreconStatusView, RankingRow } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { JobProgress } from "../components/JobProgress";
import { formatEuro } from "../format";
import { useAutoSave } from "../useAutoSave";
import { useDebounced } from "../useDebounced";
import { parseDecimal } from "./buyHelper";
import {
  coverageText,
  formatRatio,
  lowCoverage,
  nextSort,
  PRICE_COLUMNS,
  RANKING_COLUMNS,
  ratioTone,
  SEALED_COLUMNS,
} from "./preconEv";

export function PreconEvPage() {
  const settings = usePreconSettings();
  const status = usePreconStatus();
  return (
    <section className="page page--wide">
      <h1>Precon EV</h1>
      <ErrorMessage error={settings.error ?? status.error} />
      {(settings.isPending || status.isPending) && <p className="loading">Loading…</p>}
      {status.data && <LoadPanel status={status.data} />}
      {settings.data && status.data && <Ranking initial={settings.data} status={status.data} />}
    </section>
  );
}

function LoadPanel({ status }: { status: PreconStatusView }) {
  const start = useStartPreconLoad();
  if (!status.configured) {
    return (
      <p className="error" role="alert">
        No price tracker is configured on the server, so nothing can be loaded. Start the server with
        --inventory-sync-url.
      </p>
    );
  }
  const loading = status.load.state === "running";
  const busy = loading || start.isPending;
  const info = status.loaded;
  return (
    <div className="panel precon-load">
      <div className="precon-load__actions">
        <button
          type="button"
          className="button button--primary"
          disabled={busy}
          onClick={() => start.mutate({ refresh_index: false })}
        >
          {info ? "Reload" : "Load precons"}
        </button>
        <button
          type="button"
          className="button"
          disabled={busy}
          title="Re-download MTGJSON's sealed product index (about 12 MB). Happens on its own weekly or when new precons appear."
          onClick={() => start.mutate({ refresh_index: true })}
        >
          Refresh MTGJSON data
        </button>
        <JobProgress status={status.load} />
        <JobProgress status={status.sales} />
      </div>
      <ErrorMessage error={start.error} />
      {status.load.state === "failed" && (
        <p className="error" role="alert">
          {status.load.error}
        </p>
      )}
      {info ? (
        <>
          <p className="muted">
            {info.products} precons · {info.with_decklist} with decklist · MTGJSON {info.deck_list_version ?? "?"} ·
            sealed index from {info.index_date ?? "?"} · loaded {new Date(info.loaded_at).toLocaleString()}
          </p>
          {info.index_stale && (
            <p className="warning">
              MTGJSON was unreachable: the cached sealed index is used, so the newest precons may show as missing.
            </p>
          )}
          {info.catalog_truncated && (
            <p className="warning">The price tracker search hit its result limit: some precons may be missing.</p>
          )}
          {status.sales.state === "running" && (
            <p className="muted">
              The sales history is loading in the background; the foil check and Liquid EV apply when it is in (the
              first load of the day takes longest).
            </p>
          )}
          {info.sales_error && (
            <p className="warning">{info.sales_error} The foil sales check and Liquid EV are not available.</p>
          )}
        </>
      ) : (
        !loading && (
          <p className="muted">
            Nothing loaded yet. Loading fetches the precon catalog and today&apos;s prices from the price tracker and
            the decklists from MTGJSON (cached after the first time); everyone using this page shares the result.
          </p>
        )
      )}
    </div>
  );
}

function Ranking({ initial, status }: { initial: PreconSettingsView; status: PreconStatusView }) {
  const [settings, setSettings] = useState<PreconSettings>(initial.settings);
  const [cutoffText, setCutoffText] = useState(String(initial.settings.cutoff));
  const [filter, setFilter] = useState("");
  const { mutate: save } = useSavePreconSettings();

  const cutoff = parseDecimal(cutoffText);
  const cutoffOk = cutoff !== null && cutoff <= 100;
  const effective = cutoffOk ? { ...settings, cutoff } : null;
  useAutoSave(effective, initial.settings, save, savePreconSettingsOnExit);
  const querySettings = useDebounced(effective, 250);
  const queryFilter = useDebounced(filter.trim(), 250);
  const version = status.loaded?.version ?? null;
  const ranking = usePreconRanking(querySettings, queryFilter, version);

  function update(changes: Partial<PreconSettings>) {
    setSettings((current) => ({ ...current, ...changes }));
  }

  return (
    <>
      <div className="panel precon-controls">
        <label className="inline-field">
          <span>Card price</span>
          <select value={settings.field} onChange={(e) => update({ field: e.target.value as PreconSettings["field"] })}>
            {PRICE_COLUMNS.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <label
          className="inline-field"
          title="Cardmarket publishes only Trend, Low (cheapest offer) and Average for sealed products."
        >
          <span>Sealed price</span>
          <select
            value={settings.sealed_field}
            onChange={(e) => update({ sealed_field: e.target.value as PreconSettings["sealed_field"] })}
          >
            {SEALED_COLUMNS.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <span className="inline-field">
          <label htmlFor="precon-cutoff">Bulk cutoff (€)</label>
          <input
            id="precon-cutoff"
            className="number-field__input"
            value={cutoffText}
            inputMode="decimal"
            maxLength={8}
            aria-invalid={!cutoffOk}
            title="Card copies cheaper than this are left out of EV: they rarely sell. Top 5 ignores the cutoff."
            onChange={(e) => setCutoffText(e.target.value)}
          />
        </span>
        <label
          className="checkbox"
          title="Value a foil at its non-foil price unless the foil sold in the last 49 days. Removes foil premiums nobody pays."
        >
          <input
            type="checkbox"
            checked={settings.foil_needs_sales}
            onChange={(e) => update({ foil_needs_sales: e.target.checked })}
          />
          <span>Foil premium only if sold</span>
        </label>
        <span className="inline-field">
          <label htmlFor="precon-filter">Filter</label>
          <input
            id="precon-filter"
            className="text-input"
            value={filter}
            placeholder="deck or set"
            maxLength={100}
            onChange={(e) => setFilter(e.target.value)}
          />
        </span>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={settings.hide_no_decklist}
            onChange={(e) => update({ hide_no_decklist: e.target.checked })}
          />
          <span>Hide products without decklist</span>
        </label>
        {!cutoffOk && (
          <p className="error" role="alert">
            Enter a cutoff from 0 to 100.
          </p>
        )}
      </div>

      <details className="explain">
        <summary>How the values are calculated</summary>
        <p>
          EV = the card copies priced at or above the bulk cutoff, at the chosen card price, against the sealed price.
          Tokens, display commanders, sample packs and extras are left out. Liquid EV = EV with each card weighted by
          how often it sold over the last 14 days: full weight from a sale on half of the days, less below, nothing if
          unsold (market-wide, all countries and languages; cards under 0.50 € count as unsellable). Top 5 = the five
          most expensive copies, regardless of the cutoff. ⚠ = the sealed price is more than 25% below the cheapest
          offer (thin market, the ratio is overstated).
        </p>
      </details>

      <ErrorMessage error={ranking.error} />
      {ranking.data && (
        <>
          <p className="muted">
            {ranking.data.rows.length === ranking.data.total
              ? `${ranking.data.total} precons`
              : `${ranking.data.rows.length} of ${ranking.data.total} precons shown`}
          </p>
          <div className={ranking.isFetching ? "table-wrap updating" : "table-wrap"}>
            <table className="table precon-table">
              <thead>
                <tr>
                  {RANKING_COLUMNS.map((column) => {
                    const active = column.sort === settings.sort;
                    const sortKey = column.sort;
                    return (
                      <th
                        key={column.label}
                        scope="col"
                        className={column.numeric ? "num" : undefined}
                        aria-sort={active ? (settings.desc ? "descending" : "ascending") : undefined}
                      >
                        {sortKey ? (
                          <button
                            type="button"
                            className="sort-button"
                            onClick={() => update(nextSort({ sort: settings.sort, desc: settings.desc }, sortKey))}
                          >
                            {column.label}
                            <span aria-hidden="true">{active ? (settings.desc ? " ⏷" : " ⏶") : ""}</span>
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
                {ranking.data.rows.map((row) => (
                  <RankingLine key={row.id_product} row={row} />
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </>
  );
}

function RankingLine({ row }: { row: RankingRow }) {
  const tone = ratioTone(row);
  const ready = row.state === "ready";
  return (
    <tr className={ready ? undefined : "row--muted"}>
      <td>
        <Link to="/precon-ev/$id" params={{ id: String(row.id_product) }}>
          {row.name}
        </Link>
      </td>
      <td>{row.set_label}</td>
      <td className="nowrap">{row.release_date ?? "—"}</td>
      <td className="num">{formatEuro(row.sealed_price)}</td>
      <td className="num">{formatEuro(row.ev?.ev ?? null)}</td>
      <td
        className={`num ratio ratio--${tone}`}
        title={
          tone === "stale" && row.stale_sealed !== null
            ? `Nobody sells near the sealed price used; the cheapest offer is ${formatEuro(row.stale_sealed)}. The ratio is overstated.`
            : undefined
        }
      >
        {formatRatio(row.ratio)}
        {tone === "stale" && " ⚠"}
      </td>
      <td className="num">{formatEuro(row.ev?.liquid ?? null)}</td>
      <td className="num">{formatEuro(row.ev?.top5 ?? null)}</td>
      <td className={lowCoverage(row.ev) ? "num warning-text" : "num"}>{coverageText(row.ev)}</td>
      <td className="details" title={row.note_detail ?? undefined}>
        {row.note}
      </td>
    </tr>
  );
}
