// The review column of the Repricing page: what the rules do to the stock
// (key figures, filters, a paged table with export ticks), how one listing
// got its price (with a hand price), and the export of the ticked changes.

import { useState } from "react";
import {
  downloadPriceExport,
  useExportPrices,
  useRepricingListing,
  useRepricingReview,
  useResetReview,
  useSetManualPrice,
  useSetTicks,
} from "../api/queries";
import type { ReviewFilter, ReviewPage, ReviewQuery, ReviewRow, ReviewSort, RuleView } from "../api/types";
import { NumField, Stat } from "../components/Controls";
import { ErrorMessage } from "../components/ErrorMessage";
import { Modal } from "../components/Modal";
import { formatCount, formatEuro } from "../format";
import { saveDownload } from "./buyHelper";
import {
  FILTERS,
  hoursSince,
  IMPORT_STALE_HOURS,
  isBigExport,
  nextSort,
  OUTCOME_TONE,
  PAGE_SIZE,
  signedEuro,
  signedPercent,
} from "./repricing";

const COLUMNS: { label: string; sort: ReviewSort; numeric?: boolean }[] = [
  { label: "Card", sort: "name" },
  { label: "Qty", sort: "quantity", numeric: true },
  { label: "Market", sort: "market", numeric: true },
  { label: "Current", sort: "current", numeric: true },
  { label: "New", sort: "new", numeric: true },
  { label: "Change", sort: "change", numeric: true },
  { label: "Impact", sort: "impact", numeric: true },
  { label: "Rule", sort: "rule" },
];

export interface ReviewState {
  filter: ReviewFilter;
  rule: number | null;
  search: string;
  sort: ReviewSort;
  desc: boolean;
  offset: number;
}

export const FIRST_VIEW: ReviewState = {
  filter: "changes",
  rule: null,
  search: "",
  sort: "impact",
  desc: true,
  offset: 0,
};

export function queryOf(state: ReviewState, draft: boolean): ReviewQuery {
  return {
    draft,
    filter: state.filter,
    rule: state.rule ?? undefined,
    search: state.search.trim() || undefined,
    sort: state.sort,
    desc: state.desc,
    offset: state.offset,
    limit: PAGE_SIZE,
  };
}

interface ReviewProps {
  page: ReviewPage;
  fetching: boolean;
  state: ReviewState;
  /** The search as typed (the query waits until typing stops). */
  search: string;
  onSearch: (search: string) => void;
  onState: (state: ReviewState) => void;
  rules: RuleView[];
  /** Ticks, hand prices and the export (`repricing.export`). */
  canExport: boolean;
  onOpen: (pile: string) => void;
  onExport: () => void;
}

export function ReviewPanel({
  page,
  fetching,
  state,
  search,
  onSearch,
  onState,
  rules,
  canExport,
  onOpen,
  onExport,
}: ReviewProps) {
  const s = page.summary;
  const ticks = useSetTicks();
  const reset = useResetReview();
  const tickable = canExport && !page.draft;
  const set = (change: Partial<ReviewState>) => onState({ ...state, offset: 0, ...change });
  const tickShown = (ticked: boolean) =>
    ticks.mutate({
      version: page.source.version,
      target: { shown: { draft: page.draft, filter: state.filter, rule: state.rule, search: state.search } },
      ticked,
    });
  const valueChange = s.value_after - s.value_before;
  const from = page.total === 0 ? 0 : page.offset + 1;
  const to = page.offset + page.rows.length;

  return (
    <div className="repricing-review">
      <div className="stats" role="group" aria-label="What the rules do" aria-busy={fetching}>
        <Stat
          label="Changes"
          value={formatCount(s.raises + s.lowers)}
          sub={`↑ ${formatCount(s.raises)} raises · ↓ ${formatCount(s.lowers)} drops`}
        />
        <Stat
          label="For export"
          value={formatCount(s.selected)}
          sub={`${formatCount(s.selected_copies)} copies ticked`}
          tone={s.selected > 0 ? "good" : undefined}
        />
        <Stat
          label="Listed value"
          value={signedEuro(valueChange)}
          sub={`${formatEuro(s.value_before)} → ${formatEuro(s.value_after)}`}
        />
        <Stat
          label="Needs review"
          value={formatCount(s.review)}
          sub="big changes not ticked yet"
          tone={s.review > 0 ? "warning" : undefined}
        />
        {s.held_back > 0 && (
          <Stat label="Held back" value={formatCount(s.held_back)} sub="reserved for bulk lots" tone="warning" />
        )}
      </div>

      {page.draft && (
        <p className="notice">
          <strong>Your draft.</strong> These are the prices your unpublished rules would give. Ticks, hand prices and
          exports use the published rules: publish first, or switch to “Published”.
        </p>
      )}
      {(page.dropped_ticks > 0 || page.dropped_manual > 0) && (
        <p className="notice notice--warning">
          A new inventory import arrived: {formatCount(page.dropped_ticks)} of your ticks and{" "}
          {formatCount(page.dropped_manual)} hand prices were made for the previous one and no longer apply.
        </p>
      )}
      {!page.market.loaded && (
        <p className="notice notice--warning">
          No market prices are loaded yet: rules based on the market find nothing to work with.
        </p>
      )}
      {page.market.loaded && page.market.covered < s.listings && (
        <p className="muted small">
          {formatCount(page.market.covered)} of {formatCount(s.listings)} listings have a market price
          {page.market.newest_price_date && ` (prices of ${page.market.newest_price_date})`}.
        </p>
      )}
      {s.held_back > 0 && (
        <p className="muted small">
          Held back: copies of these cards are reserved for a bulk lot waiting for its import. They are never exported,
          so the new-prices file cannot bring the copies back.
        </p>
      )}

      <div className="review-filters">
        <div className="segmented" role="group" aria-label="Show">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              type="button"
              className={state.filter === f.key ? "segmented__button segmented__button--on" : "segmented__button"}
              aria-pressed={state.filter === f.key}
              onClick={() => set({ filter: f.key })}
            >
              {f.label}
            </button>
          ))}
        </div>
        <select
          className="rule-select review-filters__rule"
          aria-label="Rule"
          value={state.rule ?? ""}
          onChange={(e) => set({ rule: e.target.value === "" ? null : Number(e.target.value) })}
        >
          <option value="">All rules</option>
          {rules.map((r, i) => (
            <option key={`${i}-${r.name}`} value={i}>
              {i + 1}. {r.name}
            </option>
          ))}
        </select>
        <input
          type="search"
          className="rule-select review-filters__search"
          aria-label="Search"
          placeholder="Name or set, comma-separated"
          maxLength={200}
          value={search}
          onChange={(e) => onSearch(e.target.value)}
        />
      </div>

      {tickable && (
        <div className="review-actions">
          <button
            type="button"
            className="button button--small"
            disabled={ticks.isPending}
            onClick={() => tickShown(true)}
          >
            Tick all shown
          </button>
          <button
            type="button"
            className="button button--small"
            disabled={ticks.isPending}
            onClick={() => tickShown(false)}
          >
            Untick all shown
          </button>
          <button
            type="button"
            className="button button--small button--ghost"
            disabled={reset.isPending}
            onClick={() => reset.mutate()}
            title="Drops your ticks and hand prices"
          >
            Back to the proposals
          </button>
          <span className="review-actions__spacer" />
          <button type="button" className="button button--primary" disabled={s.selected === 0} onClick={onExport}>
            Export {formatCount(s.selected)} {s.selected === 1 ? "change" : "changes"}…
          </button>
        </div>
      )}
      <ErrorMessage error={ticks.error ?? reset.error} />

      {page.total === 0 ? (
        <p className="muted">No listings match.</p>
      ) : (
        <div className={fetching ? "table-wrap updating" : "table-wrap"}>
          <table className="table review-table" aria-label="Proposed prices">
            <thead>
              <tr>
                <th scope="col">
                  <span className="visually-hidden">Export</span>
                </th>
                {COLUMNS.map((c) => {
                  const active = c.sort === state.sort;
                  return (
                    <th
                      key={c.sort}
                      scope="col"
                      className={c.numeric ? "num" : undefined}
                      aria-sort={active ? (state.desc ? "descending" : "ascending") : undefined}
                    >
                      <button
                        type="button"
                        className="sort-button"
                        onClick={() => set(nextSort({ sort: state.sort, desc: state.desc }, c.sort))}
                      >
                        {c.label}
                        <span aria-hidden="true">{active ? (state.desc ? " ⏷" : " ⏶") : ""}</span>
                      </button>
                    </th>
                  );
                })}
                <th scope="col">Why</th>
              </tr>
            </thead>
            <tbody>
              {page.rows.map((row) => (
                <Row
                  key={row.pile}
                  row={row}
                  rules={rules}
                  tickable={tickable}
                  onTick={(ticked) =>
                    ticks
                      .mutateAsync({ version: page.source.version, target: { piles: [row.pile] }, ticked })
                      .catch(() => undefined)
                  }
                  onOpen={() => onOpen(row.pile)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
      {page.total > PAGE_SIZE && (
        <div className="pager">
          <button
            type="button"
            className="button button--small"
            disabled={page.offset === 0}
            onClick={() => onState({ ...state, offset: Math.max(0, page.offset - PAGE_SIZE) })}
          >
            ⏴ Previous
          </button>
          <span className="muted">
            {formatCount(from)}–{formatCount(to)} of {formatCount(page.total)}
          </span>
          <button
            type="button"
            className="button button--small"
            disabled={to >= page.total}
            onClick={() => onState({ ...state, offset: page.offset + PAGE_SIZE })}
          >
            Next ⏵
          </button>
        </div>
      )}
    </div>
  );
}

function Row({
  row,
  rules,
  tickable,
  onTick,
  onOpen,
}: {
  row: ReviewRow;
  rules: RuleView[];
  tickable: boolean;
  /** Settles when the server has the tick. */
  onTick: (ticked: boolean) => Promise<unknown>;
  onOpen: () => void;
}) {
  const change = row.outcome === "raise" || row.outcome === "lower";
  // The clicked state, until the server has it (the box must not jump back).
  const [clicked, setClicked] = useState<boolean | null>(null);
  const tone = OUTCOME_TONE[row.outcome];
  const toneClass = tone === "up" ? "change--up" : tone === "down" ? "change--down" : "";
  const rule = row.rule === null ? null : rules[row.rule];
  return (
    <tr className={change ? undefined : "row--muted"}>
      <td>
        {change && (
          <input
            type="checkbox"
            className="review-tick"
            checked={clicked ?? row.ticked}
            disabled={!tickable || row.held_back}
            aria-label={`Export the new price of ${row.name}`}
            onChange={(e) => {
              setClicked(e.target.checked);
              void onTick(e.target.checked).finally(() => setClicked(null));
            }}
          />
        )}
      </td>
      <td>
        <button type="button" className="link-button review-table__name" onClick={onOpen}>
          {row.name}
          {row.foil && " ✦"}
        </button>
        <span className="review-table__sub">
          {row.set_code}
          {row.collector_number && ` #${row.collector_number}`} · {row.condition} · {row.language}
        </span>
      </td>
      <td className="num">{row.quantity}</td>
      <td className="num">{formatEuro(row.market)}</td>
      <td className="num">{formatEuro(row.old_price)}</td>
      <td className={`num ${toneClass}`}>{change ? <strong>{formatEuro(row.new_price)}</strong> : "—"}</td>
      <td className={`num ${toneClass}`}>{change ? signedPercent(row.delta_pct) : ""}</td>
      <td className={`num ${toneClass}`}>{change ? signedEuro((row.new_price - row.old_price) * row.quantity) : ""}</td>
      <td>
        {row.manual ? (
          <span className="muted small">by hand</span>
        ) : rule && row.rule !== null ? (
          <span className="rule-badge" title={rule.name}>
            {row.rule + 1}
            <span className="visually-hidden"> {rule.name}</span>
          </span>
        ) : (
          "—"
        )}
      </td>
      <td>
        {change ? (row.outcome === "raise" ? "Raise" : "Drop") : row.outcome_label}
        {row.review && !row.ticked && <span className="badge badge--warning">check</span>}
        {row.held_back && <span className="badge badge--warning">held back</span>}
      </td>
    </tr>
  );
}

// ── One listing ─────────────────────────────────────────────────────────────

export function ListingDialog({
  pile,
  draft,
  draftKey,
  version,
  canExport,
  onClose,
}: {
  pile: string;
  draft: boolean;
  draftKey: string;
  version: number;
  canExport: boolean;
  onClose: () => void;
}) {
  const detail = useRepricingListing(pile, draft, draftKey);
  const manual = useSetManualPrice();
  const d = detail.data;
  const [price, setPrice] = useState<number | null>(null);
  const change = d && (d.row.outcome === "raise" || d.row.outcome === "lower");
  return (
    <Modal title={d?.row.name ?? "Card"} onClose={onClose}>
      <ErrorMessage error={detail.error} />
      {d && (
        <div className="listing-detail">
          <p className="muted">
            {d.row.set_name} ({d.row.set_code}
            {d.row.collector_number && ` #${d.row.collector_number}`}) · {d.row.condition} · {d.row.language}
            {d.row.foil && " · foil"} · ×{d.row.quantity} · {d.row.location || "no location"}
          </p>
          <div className="stats">
            <Stat label="Current" value={formatEuro(d.row.old_price)} sub={d.row.rarity || "—"} />
            <Stat
              label="New"
              value={change ? formatEuro(d.row.new_price) : "unchanged"}
              sub={change ? signedPercent(d.row.delta_pct) : d.row.outcome_label}
              tone={change ? "good" : undefined}
            />
            <Stat
              label="Market"
              value={formatEuro(d.row.market)}
              sub={d.market_date ? `prices of ${d.market_date}` : "no row"}
            />
          </div>
          <h3 className="rule-group__title">How the price was found</h3>
          <ol className="listing-detail__steps">
            {d.steps.map((step, i) => (
              <li key={i}>{step}</li>
            ))}
          </ol>
          {d.row.held_back && (
            <p className="notice notice--warning">
              Copies of this card are reserved for a bulk lot waiting for its import, so it is never exported.
            </p>
          )}
          <div className="listing-detail__facts">
            <div>
              <h3 className="rule-group__title">Market</h3>
              {d.market.length === 0 ? (
                <p className="muted small">No market row for this card.</p>
              ) : (
                <table className="table table--compact" aria-label="Market prices">
                  <tbody>
                    {d.market.map((m) => (
                      <tr key={m.label}>
                        <th scope="row">{m.label}</th>
                        <td className="num">{formatEuro(m.price)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
            <div>
              <h3 className="rule-group__title">Sales</h3>
              <p className="small">
                In stock for {d.age_days === null ? "—" : `${d.age_days} days`} · {d.sold_30d} sold in 30 days
                {d.last_sale && ` · last sale ${d.last_sale}`}
              </p>
              {d.sales.length > 0 && (
                <ul className="listing-detail__sales">
                  {d.sales.map((s) => (
                    <li key={s.date}>
                      {s.date} <span className="muted">×{s.copies}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
          {canExport && (
            <div className="listing-detail__manual">
              <h3 className="rule-group__title">Price by hand</h3>
              <p className="muted small">Wins over every rule, for your review only.</p>
              <div className="import-actions">
                <NumField
                  id="manual-price"
                  label="Price by hand (€)"
                  value={price ?? d.manual_price}
                  rules={{ min: 0.02, max: 100_000, optional: true }}
                  unit="€"
                  onChange={setPrice}
                />
                <button
                  type="button"
                  className="button button--primary"
                  disabled={manual.isPending || (price ?? d.manual_price) === null}
                  onClick={() => manual.mutate({ version, pile, price: price ?? d.manual_price })}
                >
                  Use this price
                </button>
                {d.manual_price !== null && (
                  <button
                    type="button"
                    className="button button--ghost"
                    disabled={manual.isPending}
                    onClick={() => {
                      setPrice(null);
                      manual.mutate({ version, pile, price: null });
                    }}
                  >
                    Back to the rules
                  </button>
                )}
              </div>
              <ErrorMessage error={manual.error} />
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}

// ── Export ──────────────────────────────────────────────────────────────────

export function ExportDialog({ draftKey, onClose }: { draftKey: string; onClose: () => void }) {
  // Always the published rules, whatever the page shows.
  const check = useRepricingReview(
    { draft: false, filter: "changes", sort: "impact", desc: true, offset: 0, limit: 5 },
    draftKey,
  );
  const exporter = useExportPrices();
  const [saved, setSaved] = useState<string[] | null>(null);
  const [downloadError, setDownloadError] = useState<unknown>(null);
  const p = check.data;

  const run = () => {
    if (!p) return;
    exporter.mutate(
      { revision: p.revision, data_version: p.data_version, market_version: p.market.version },
      {
        onSuccess: async (result) => {
          try {
            const prices = await downloadPriceExport(result.id, false, result.file_name);
            saveDownload(prices);
            const undo = await downloadPriceExport(result.id, true, result.undo_file_name);
            saveDownload(undo);
            setSaved([result.file_name, result.undo_file_name]);
          } catch (e) {
            setDownloadError(e);
          }
        },
      },
    );
  };

  const s = p?.summary;
  const hours = p ? hoursSince(p.source.imported_at, new Date()) : 0;
  return (
    <Modal title="Export new prices" onClose={onClose}>
      <ErrorMessage error={check.error} />
      {p && s && !saved && (
        <>
          <p>
            <strong>
              {formatCount(s.selected)} price {s.selected === 1 ? "change" : "changes"}
            </strong>{" "}
            ({formatCount(s.selected_copies)} copies) by the published rules (revision {p.revision}), for the import{" "}
            {p.source.file_name} of {new Date(p.source.imported_at).toLocaleString()}.
          </p>
          {hours > IMPORT_STALE_HOURS && (
            <p className="notice notice--warning" role="alert">
              That import is {Math.floor(hours / 24)} {Math.floor(hours / 24) === 1 ? "day" : "days"} old. The file
              carries each card’s copies as of that import; if the price import also sets the stock, copies sold since
              would come back. Import a fresh inventory export first if you can.
            </p>
          )}
          {isBigExport(s.selected, s.listings) && (
            <p className="notice notice--warning" role="alert">
              This changes {Math.round((s.selected / s.listings) * 100)}% of all {formatCount(s.listings)} listings.
              Check the biggest changes below before you upload the file.
            </p>
          )}
          {s.held_back > 0 && (
            <p className="muted small">
              {formatCount(s.held_back)} {s.held_back === 1 ? "change is" : "changes are"} held back for bulk lots and
              not in the file.
            </p>
          )}
          {p.rows.length > 0 && (
            <>
              <h3 className="rule-group__title">Biggest changes</h3>
              <table className="table table--compact" aria-label="Biggest changes">
                <tbody>
                  {p.rows.map((r) => (
                    <tr key={r.pile}>
                      <td>
                        {r.name} <span className="muted">({r.set_code})</span>
                      </td>
                      <td className="num">
                        {formatEuro(r.old_price)} → {formatEuro(r.new_price)}
                      </td>
                      <td className="num">×{r.quantity}</td>
                      <td className="num">{r.ticked ? "ticked" : "not ticked"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
          <p className="small">
            You get two files: the new prices and an <code>_undo.csv</code> with the current ones, to put them back.
            Both stay downloadable under “Exports”. Try the import with a few cards first.
          </p>
          <div className="import-actions">
            <button
              type="button"
              className="button button--primary"
              disabled={s.selected === 0 || exporter.isPending}
              onClick={run}
            >
              Export and download both files
            </button>
            <button type="button" className="button button--ghost" onClick={onClose}>
              Cancel
            </button>
          </div>
          <ErrorMessage error={exporter.error ?? downloadError} />
        </>
      )}
      {saved && (
        <>
          <p className="notice notice--success">
            Saved <code>{saved[0]}</code> and <code>{saved[1]}</code>.
          </p>
          <div className="import-actions">
            <button type="button" className="button" onClick={onClose}>
              Close
            </button>
          </div>
        </>
      )}
    </Modal>
  );
}
