// Mispricing: every listing in stock against the market, with what to do
// about it ("raise now", "cut", …), sorted by what to fix first. Plus the
// internal consistency check: our own listings that contradict each other.
// Market data comes from the shared, once-a-day load on the server. Money
// totals only appear for users allowed to see them.

import { useState } from "react";
import { useMarketStatus, useMispricing, type MispricingParams } from "../api/queries";
import type {
  ConflictKind,
  MarketStatusView,
  MispricingRow,
  MispricingSortKey,
  MispricingSummary,
  PriceAction,
  PriceColumn,
  Verdict,
  VerdictFilterKey,
} from "../api/types";
import { DataAsOf } from "../components/DataAsOf";
import { ErrorMessage } from "../components/ErrorMessage";
import { MarketPanel } from "../components/MarketPanel";
import { formatCount, formatEuro } from "../format";
import { useDebounced } from "../useDebounced";
import { parseDecimal } from "./buyHelper";
import { changeClass, formatChange } from "./MoversPage";
import { isCardmarketUrl, PRICE_COLUMNS } from "./preconEv";

export const VERDICT_LABELS: Record<Verdict, string> = {
  underpriced: "Underpriced",
  overpriced: "Overpriced",
  fair: "Fair",
  no_market_data: "No market data",
};

export const ACTION_LABELS: Record<PriceAction, string> = {
  raise_now: "Raise now",
  raise: "Raise",
  watch: "Watch",
  cut_now: "Cut now",
  cut: "Cut",
  hold: "Hold",
  none: "—",
};

export const CONFLICT_LABELS: Record<ConflictKind, string> = {
  condition_inversion: "Condition inversion",
  foil_inversion: "Foil below non-foil",
  duplicate_price: "Duplicate at different price",
};

const FILTERS: { value: VerdictFilterKey; label: string }[] = [
  { value: "all", label: "All" },
  { value: "underpriced", label: "Underpriced" },
  { value: "overpriced", label: "Overpriced" },
  { value: "no_data", label: "No data" },
];

const COLUMNS: { label: string; sort: MispricingSortKey; numeric?: boolean }[] = [
  { label: "Card", sort: "name" },
  { label: "Set", sort: "set" },
  { label: "Qty", sort: "quantity", numeric: true },
  { label: "Age", sort: "age", numeric: true },
  { label: "Listed", sort: "listed", numeric: true },
  { label: "Market", sort: "market", numeric: true },
  { label: "Δ7d", sort: "change7", numeric: true },
  { label: "Δ30d", sort: "change30", numeric: true },
  { label: "Δ%", sort: "delta_pct", numeric: true },
  { label: "Impact", sort: "impact", numeric: true },
  { label: "Verdict", sort: "verdict" },
  { label: "Action", sort: "priority" },
];

/** Same column: flip; another: descending (text columns ascending). */
export function nextMispricingSort(current: { sort: MispricingSortKey; desc: boolean }, column: MispricingSortKey) {
  if (current.sort === column) return { sort: column, desc: !current.desc };
  return { sort: column, desc: !["name", "set", "verdict"].includes(column) };
}

/** The verdict with its flags (below the cheapest offer, new listing, language), and why. */
export function verdictText(row: MispricingRow): { text: string; hint: string } {
  let text = VERDICT_LABELS[row.verdict];
  const hints: string[] = [];
  if (row.below_low) {
    text += " ▾low";
    hints.push("Listed below the market's cheapest listing.");
  }
  if (row.recently_listed) {
    text += " · new";
    hints.push("Listed within the last day: check for a typo.");
  }
  if (row.language_flagged) {
    text += " · lang";
    hints.push(
      `${row.language} copy: the language-blind reference likely overstates its value; underpriced verdicts are doubtful.`,
    );
  }
  return { text, hint: hints.join(" ") };
}

/** The market price with its flags (condition-adjusted *, stale ⚠), and why. */
export function marketText(row: MispricingRow): { text: string; hint: string } {
  if (row.market === null) return { text: "—", hint: "" };
  let text = formatEuro(row.market);
  const hints: string[] = [];
  if (row.condition_factor < 1) {
    text += "*";
    hints.push(`Reference ×${row.condition_factor.toFixed(2)} for ${row.condition} condition (guide price ≈ NM).`);
  }
  if (row.stale) {
    text += " ⚠";
    hints.push("Market data is more than a week old: treat with caution.");
  }
  return { text, hint: hints.join(" ") };
}

export function actionClass(action: PriceAction): string {
  switch (action) {
    case "raise_now":
      return "action action--up action--now";
    case "raise":
      return "action action--up";
    case "cut_now":
      return "action action--down action--now";
    case "cut":
      return "action action--down";
    default:
      return "action muted";
  }
}

export function MispricingPage() {
  const status = useMarketStatus("/api/mispricing");
  return (
    <section className="page page--wide">
      <h1>Mispricing</h1>
      <DataAsOf />
      <ErrorMessage error={status.error} />
      {status.data && <Mispricing status={status.data} />}
    </section>
  );
}

function Mispricing({ status }: { status: MarketStatusView }) {
  const [field, setField] = useState<PriceColumn>("trend");
  const [thresholdText, setThresholdText] = useState("15");
  const [filter, setFilter] = useState<VerdictFilterKey>("all");
  const [order, setOrder] = useState<{ sort: MispricingSortKey; desc: boolean }>({ sort: "priority", desc: true });
  const threshold = parseDecimal(thresholdText);
  const thresholdOk = threshold !== null && threshold <= 100;
  const params: MispricingParams | null = thresholdOk ? { field, threshold, filter, ...order } : null;
  const settled = useDebounced(params, 250);
  const report = useMispricing(settled, status.loaded?.version ?? null);

  if (!status.configured) return <MarketPanel base="/api/mispricing" status={status} />;

  return (
    <>
      <MarketPanel base="/api/mispricing" status={status} />
      <div className="panel precon-controls">
        <label className="inline-field">
          <span>Reference price</span>
          <select value={field} onChange={(e) => setField(e.target.value as PriceColumn)}>
            {PRICE_COLUMNS.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <span className="inline-field">
          <label htmlFor="mispricing-threshold">Fair band ± (%)</label>
          <input
            id="mispricing-threshold"
            className="number-field__input"
            value={thresholdText}
            inputMode="decimal"
            maxLength={6}
            aria-invalid={!thresholdOk}
            title="Listings within this band of the market count as fair; a card's own volatility can widen it."
            onChange={(e) => setThresholdText(e.target.value)}
          />
        </span>
        {!thresholdOk && (
          <p className="error" role="alert">
            Enter a band from 0 to 100 %.
          </p>
        )}
      </div>

      <ErrorMessage error={report.error} />
      {report.data && (
        <>
          <Summary summary={report.data.summary} />
          <details className="consistency">
            <summary className={report.data.conflicts_total ? "warning-text" : "success"}>
              {report.data.conflicts_total
                ? `Internal consistency: ${report.data.conflicts_total} conflicts`
                : "Internal consistency ✓ (no conflicts)"}
            </summary>
            {report.data.conflicts_total === 0 ? (
              <p className="muted">
                No listings contradict each other (condition order, foil premium, duplicate prices).
              </p>
            ) : (
              <>
                {report.data.conflicts_total > report.data.conflicts.length && (
                  <p className="muted">Showing the first {report.data.conflicts.length} conflicts.</p>
                )}
                <div className="table-wrap">
                  <table className="table">
                    <thead>
                      <tr>
                        <th scope="col">Type</th>
                        <th scope="col">Card</th>
                        <th scope="col">Listings</th>
                      </tr>
                    </thead>
                    <tbody>
                      {report.data.conflicts.map((c, i) => (
                        <tr key={`${i}-${c.name}`}>
                          <td className="nowrap">{CONFLICT_LABELS[c.kind]}</td>
                          <td>
                            {c.name} ({c.set_code})
                          </td>
                          <td className="details">{c.details}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            )}
          </details>

          <div className="report-toolbar">
            <span className="segmented" role="group" aria-label="Show">
              {FILTERS.map((f) => (
                <button
                  key={f.value}
                  type="button"
                  className={filter === f.value ? "segmented__button segmented__button--on" : "segmented__button"}
                  aria-pressed={filter === f.value}
                  onClick={() => setFilter(f.value)}
                >
                  {f.label}
                </button>
              ))}
            </span>
            <span className="muted">
              {report.data.total > report.data.rows.length
                ? `Showing the first ${report.data.rows.length} of ${formatCount(report.data.total)} listings`
                : `${formatCount(report.data.total)} listings`}
            </span>
          </div>
          <div className={report.isFetching ? "table-wrap updating" : "table-wrap"}>
            <table className="table mispricing-table">
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
                          onClick={() => setOrder((current) => nextMispricingSort(current, column.sort))}
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
                {report.data.rows.map((row, index) => (
                  <Row key={`${index}-${row.name}-${row.set_code}-${row.condition}-${row.location}`} row={row} />
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </>
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

function Summary({ summary: s }: { summary: MispricingSummary }) {
  return (
    <>
      <div className="kpis">
        <Kpi label="Underpriced" value={`${formatCount(s.underpriced_rows)} · ×${formatCount(s.underpriced_copies)}`} />
        <Kpi label="Overpriced" value={`${formatCount(s.overpriced_rows)} · ×${formatCount(s.overpriced_copies)}`} />
        <Kpi label="Fair" value={formatCount(s.fair_rows)} />
        <Kpi label="No market data" value={formatCount(s.no_data_rows)} />
        <Kpi label="Act now" value={`${s.raise_now_rows} raise · ${s.cut_now_rows} cut`} />
        <Kpi label="Stale market data" value={formatCount(s.stale_rows)} />
        {s.upside !== null && <Kpi label="Upside" value={formatEuro(s.upside)} tone="good" />}
        {s.excess !== null && <Kpi label="Above market" value={formatEuro(s.excess)} tone="warning" />}
        {s.listed_value !== null && <Kpi label="Listed value*" value={formatEuro(s.listed_value)} />}
        {s.market_value !== null && <Kpi label="Market value*" value={formatEuro(s.market_value)} />}
      </div>
      {s.listed_value !== null && <p className="muted small">* Only listings with market data.</p>}
    </>
  );
}

function Row({ row }: { row: MispricingRow }) {
  const verdict = verdictText(row);
  const market = marketText(row);
  const hasMarket = row.market !== null;
  return (
    <tr>
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
      <td className="num">×{row.quantity}</td>
      <td className="num">{row.age_days} d</td>
      <td className="num">{formatEuro(row.listed)}</td>
      <td className={row.stale ? "num muted" : "num"} title={market.hint || undefined}>
        {market.text}
      </td>
      <td className={changeClass(row.pct_7d)}>{formatChange(row.pct_7d)}</td>
      <td className={changeClass(row.pct_30d)}>{formatChange(row.pct_30d)}</td>
      <td
        className={`num verdict--${row.verdict}`}
        title={hasMarket ? `Fair band ±${Math.round(row.effective_threshold_pct)}%` : undefined}
      >
        {hasMarket
          ? `${row.delta_pct > 0 ? "+" : row.delta_pct < 0 ? "−" : "±"}${Math.abs(row.delta_pct).toFixed(0)}%`
          : "—"}
      </td>
      <td className="num">{hasMarket ? formatEuro(row.impact) : "—"}</td>
      <td className={`nowrap verdict--${row.verdict}`} title={verdict.hint || undefined}>
        {verdict.text}
      </td>
      <td className={actionClass(row.action)}>{ACTION_LABELS[row.action]}</td>
    </tr>
  );
}
