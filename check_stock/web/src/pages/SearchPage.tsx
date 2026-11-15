// Search the stock of the last import (every pile with its own location),
// see a card's price history, and discard junk: the copies leave the stock
// without counting as sold, and a stock-update CSV for Cardmarket is
// downloaded. The server takes every value of the CSV from its own data
// and refuses when fewer copies are left than asked for.

import { Link, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import {
  can,
  downloadDiscardCsv,
  useCardHistory,
  useDiscard,
  useDiscardHistory,
  useMe,
  useSearch,
  type SearchField,
  type SearchParams,
} from "../api/queries";
import type { DiscardResult, PileRow } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { LineChart } from "../components/LineChart";
import { Modal } from "../components/Modal";
import { formatCount, formatEuro } from "../format";
import { markSeen } from "../inventoryVersion";
import { useDebounced } from "../useDebounced";
import { sourceText } from "./bins";
import { saveDownload } from "./buyHelper";
import { isCardmarketUrl } from "./preconEv";
import { addToSelection, DISCARD_STEPS, formatPct, selectionTotals, type Selected } from "./searchPage";
import { handOver } from "./stockChecker";
import { formatTime } from "./UsersPage";

const FIELDS: { key: SearchField; label: string }[] = [
  { key: "name", label: "Name" },
  { key: "set", label: "Set" },
  { key: "condition", label: "Condition" },
  { key: "language", label: "Language" },
  { key: "location", label: "Location" },
  { key: "rarity", label: "Rarity" },
  { key: "price", label: "Price" },
  { key: "comment", label: "Comment" },
];

export function SearchPage() {
  const { data: me } = useMe();
  const mayDiscard = can(me, "inventory.discard");
  const money = can(me, "financials.view");
  const [text, setText] = useState("");
  const [fields, setFields] = useState<SearchField[]>(["name", "set"]);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [allLanguages, setAllLanguages] = useState(false);
  const [perPage, setPerPage] = useState(100);
  const [page, setPage] = useState(0);
  const q = useDebounced(text.trim(), 300);
  const params: SearchParams = { q, fields, caseSensitive, allLanguages, page, perPage };
  const search = useSearch(params);
  const [selection, setSelection] = useState<Selected[]>([]);
  const [history, setHistory] = useState<PileRow | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [done, setDone] = useState<DiscardResult | null>(null);
  const discard = useDiscard();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const data = search.data;
  const pages = data ? Math.max(1, Math.ceil(data.total / data.per_page)) : 1;

  function toggleField(key: SearchField) {
    setPage(0);
    setFields((current) => (current.includes(key) ? current.filter((f) => f !== key) : [...current, key]));
  }

  function runDiscard() {
    discard.mutate(
      { items: selection.map((s) => ({ pile: s.pile.id, quantity: s.quantity })) },
      {
        onSuccess: (result) => {
          setConfirming(false);
          setDone(result);
          setSelection([]);
          markSeen(result.data_version);
          void queryClient.invalidateQueries();
          void downloadDiscardCsv(result.id).then(saveDownload);
        },
      },
    );
  }

  return (
    <section className="page page--wide">
      <h1>Search</h1>
      {data && !data.source && (
        <p className="notice notice--warning">
          Search needs an inventory import on the server.{" "}
          {can(me, "inventory.import") ? <Link to="/inventory-import">Import the inventory</Link> : null}
        </p>
      )}
      {data?.source && <p className="data-as-of">{sourceText(data.source)}</p>}

      <div className="panel search-controls">
        <label className="search-controls__text">
          <span className="visually-hidden">Search</span>
          <input
            type="search"
            className="input"
            placeholder="Search…"
            maxLength={200}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setPage(0);
            }}
            aria-label="Search"
          />
        </label>
        <fieldset className="search-controls__fields">
          <legend>Search in</legend>
          {FIELDS.map((f) => (
            <label key={f.key} className="checkbox">
              <input type="checkbox" checked={fields.includes(f.key)} onChange={() => toggleField(f.key)} />
              {f.label}
            </label>
          ))}
        </fieldset>
        <label className="checkbox">
          <input type="checkbox" checked={caseSensitive} onChange={(e) => setCaseSensitive(e.target.checked)} />
          Case sensitive
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={allLanguages} onChange={(e) => setAllLanguages(e.target.checked)} />
          Also German, Spanish, French and Italian names
        </label>
        <span className="inline-field">
          <label htmlFor="search-per-page">Per page</label>
          <select
            id="search-per-page"
            value={perPage}
            onChange={(e) => {
              setPerPage(Number(e.target.value));
              setPage(0);
            }}
          >
            {[25, 50, 100, 200].map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </span>
      </div>
      <ErrorMessage error={search.error} />

      {done && (
        <div className="notice notice--success" role="status">
          <p>
            Discarded {formatCount(done.copies)} {done.copies === 1 ? "copy" : "copies"}
            {done.value !== null && ` (listed at ${formatEuro(done.value)})`}. Import the downloaded CSV into TCG
            Powertools now, then import a fresh export here.
          </p>
          <button
            type="button"
            className="button button--small"
            onClick={() => void downloadDiscardCsv(done.id).then(saveDownload)}
          >
            Download the CSV again
          </button>
        </div>
      )}

      {selection.length > 0 && (
        <SelectionPanel
          selection={selection}
          money={money}
          mayDiscard={mayDiscard}
          mayCheck={can(me, "stock_checker.use")}
          onChange={setSelection}
          onDiscard={() => setConfirming(true)}
          onSend={() => {
            handOver(selection);
            void navigate({ to: "/stock-checker" });
          }}
        />
      )}

      {data?.source && (
        <>
          <p className="muted">
            {formatCount(data.total)} piles · {formatCount(data.copies)} copies
          </p>
          <div className="table-wrap">
            <table className="table search-table" aria-label="Results">
              <thead>
                <tr>
                  <th scope="col">Card</th>
                  <th scope="col">Set</th>
                  <th scope="col">Cond.</th>
                  <th scope="col">Language</th>
                  <th scope="col" className="num">
                    Qty
                  </th>
                  <th scope="col" className="num">
                    Price
                  </th>
                  <th scope="col">Location</th>
                  <th scope="col">
                    <span className="visually-hidden">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {data.rows.map((r) => (
                  <ResultRow
                    key={r.id}
                    row={r}
                    selected={selection.find((s) => s.pile.id === r.id)?.quantity ?? 0}
                    onAdd={(quantity) => setSelection((s) => addToSelection(s, r, quantity))}
                    onHistory={() => setHistory(r)}
                  />
                ))}
              </tbody>
            </table>
          </div>
          {pages > 1 && (
            <div className="pager">
              <button type="button" className="button" disabled={page === 0} onClick={() => setPage(page - 1)}>
                Previous
              </button>
              <span>
                Page {page + 1} of {pages}
              </span>
              <button type="button" className="button" disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>
                Next
              </button>
            </div>
          )}
        </>
      )}

      {mayDiscard && <DiscardHistory />}

      {history && <HistoryDialog row={history} onClose={() => setHistory(null)} />}
      {confirming && (
        <Modal title="Import order matters for discards" onClose={() => setConfirming(false)}>
          <DiscardWarning selection={selection} money={money} />
          <ErrorMessage error={discard.error} />
          <div className="import-actions">
            <button type="button" className="button button--danger" disabled={discard.isPending} onClick={runDiscard}>
              {discard.isPending ? "Discarding…" : "Discard and download the CSV"}
            </button>
            <button type="button" className="button button--ghost" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}

function ResultRow({
  row: r,
  selected,
  onAdd,
  onHistory,
}: {
  row: PileRow;
  selected: number;
  onAdd: (quantity: number) => void;
  onHistory: () => void;
}) {
  const left = r.quantity - selected;
  return (
    <tr>
      <td>
        {r.cardmarket_url && isCardmarketUrl(r.cardmarket_url) ? (
          <a href={r.cardmarket_url} target="_blank" rel="noopener noreferrer">
            {r.name}
          </a>
        ) : (
          r.name
        )}
        {r.foil && " ✦"}
        {r.signed && " ✍"}
        {r.comment && <span className="muted small"> · {r.comment}</span>}
      </td>
      <td className="nowrap">
        {r.set_code} {r.collector_number && `#${r.collector_number}`}
      </td>
      <td>{r.condition}</td>
      <td>{r.language}</td>
      <td className="num">×{r.quantity}</td>
      <td className="num">{formatEuro(r.price)}</td>
      <td className="nowrap">{r.location || "—"}</td>
      <td className="nowrap search-table__actions">
        {r.product_id !== null && (
          <button
            type="button"
            className="button button--small"
            onClick={onHistory}
            aria-label={`Price history of ${r.name}`}
          >
            📈
          </button>
        )}
        <button
          type="button"
          className="button button--small"
          disabled={left <= 0}
          onClick={() => onAdd(1)}
          aria-label={`Select one ${r.name}`}
        >
          {selected > 0 ? `+1 (${selected})` : "+1"}
        </button>
        {r.quantity > 1 && (
          <button
            type="button"
            className="button button--small"
            disabled={left <= 0}
            onClick={() => onAdd(left)}
            aria-label={`Select all ${r.name}`}
          >
            All
          </button>
        )}
      </td>
    </tr>
  );
}

function SelectionPanel({
  selection,
  money,
  mayDiscard,
  mayCheck,
  onChange,
  onDiscard,
  onSend,
}: {
  selection: Selected[];
  money: boolean;
  mayDiscard: boolean;
  mayCheck: boolean;
  onChange: (selection: Selected[]) => void;
  onDiscard: () => void;
  onSend: () => void;
}) {
  const totals = selectionTotals(selection);
  return (
    <section className="panel" aria-label="Selection">
      <p className="panel__title">
        Selected: {formatCount(totals.copies)} {totals.copies === 1 ? "copy" : "copies"} in {selection.length}{" "}
        {selection.length === 1 ? "pile" : "piles"}
        {money && ` · listed at ${formatEuro(totals.value)}`}
      </p>
      <ul className="selection-list">
        {selection.map((s) => (
          <li key={s.pile.id}>
            <span>
              ×{s.quantity} {s.pile.name} ({s.pile.set_code}, {s.pile.condition}, {s.pile.language}) ·{" "}
              {s.pile.location || "—"}
            </span>
            <button
              type="button"
              className="button button--ghost button--small"
              onClick={() => onChange(selection.filter((x) => x.pile.id !== s.pile.id))}
              aria-label={`Remove ${s.pile.name}`}
            >
              ✕
            </button>
          </li>
        ))}
      </ul>
      <div className="import-actions">
        {mayCheck && (
          <button type="button" className="button button--primary" onClick={onSend}>
            Send to Stock Checker
          </button>
        )}
        {mayDiscard && (
          <button type="button" className="button button--danger" onClick={onDiscard}>
            Discard…
          </button>
        )}
        <button type="button" className="button button--ghost" onClick={() => onChange([])}>
          Clear selection
        </button>
      </div>
    </section>
  );
}

function DiscardWarning({ selection, money }: { selection: Selected[]; money: boolean }) {
  const totals = selectionTotals(selection);
  return (
    <>
      <p>
        You are writing off {formatCount(totals.copies)} {totals.copies === 1 ? "copy" : "copies"}
        {money && ` listed at ${formatEuro(totals.value)}`}. They leave the stock now and are not counted as sold.
      </p>
      <p className="muted">
        Cardmarket keeps listing them until you import the CSV. An inventory export that still contains them is refused
        by the import check (it would bring them back as stock, and their later disappearance would count as sales).
      </p>
      <p>
        <strong>Do it in this order</strong>
      </p>
      <ol>
        {DISCARD_STEPS.map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>
      <p className="muted small">A backup of the database is taken right before the discard.</p>
    </>
  );
}

function HistoryDialog({ row, onClose }: { row: PileRow; onClose: () => void }) {
  const history = useCardHistory(row.product_id === null ? null : { product: row.product_id, foil: row.foil });
  const data = history.data;
  return (
    <Modal title={`Price history — ${row.name}${row.foil ? " ✦" : ""}`} onClose={onClose}>
      {history.isPending && <p className="loading">Fetching history…</p>}
      <ErrorMessage error={history.error} />
      {data && data.points.length < 2 && <p className="muted">Not enough price history for this card yet.</p>}
      {data && data.points.length >= 2 && (
        <>
          <p className="muted small">
            {data.name}
            {data.expansion && ` · ${data.expansion}`}
          </p>
          <LineChart
            label={`Trend price of ${data.name}`}
            dates={data.points.map((p) => p.date)}
            series={[
              {
                id: "trend",
                label: data.foil ? "Trend (foil)" : "Trend",
                className: "chart-series--ev",
                values: data.points.map((p) => p.price),
              },
            ]}
            format={(v) => formatEuro(v)}
            height={200}
          />
          <p>
            Trend <strong>{formatEuro(data.trend)}</strong> · 7 days {formatPct(data.change_7d)} · 30 days{" "}
            {formatPct(data.change_30d)} · {data.points.length} days shown
          </p>
        </>
      )}
    </Modal>
  );
}

function DiscardHistory() {
  const history = useDiscardHistory(true);
  if (!history.data || history.data.length === 0) return null;
  return (
    <>
      <h2>Recent discards</h2>
      <div className="table-wrap">
        <table className="table" aria-label="Recent discards">
          <thead>
            <tr>
              <th scope="col">When</th>
              <th scope="col">By</th>
              <th scope="col" className="num">
                Copies
              </th>
              <th scope="col">State</th>
              <th scope="col">CSV</th>
            </tr>
          </thead>
          <tbody>
            {history.data.map((d) => (
              <tr key={d.id}>
                <td className="nowrap">{formatTime(d.created_at)}</td>
                <td>{d.user_name}</td>
                <td className="num">×{d.copies}</td>
                <td>{d.settled ? "Settled by an import" : "Waiting for the next import"}</td>
                <td>
                  {d.csv_available ? (
                    <button
                      type="button"
                      className="button button--small"
                      onClick={() => void downloadDiscardCsv(d.id).then(saveDownload)}
                    >
                      Download
                    </button>
                  ) : (
                    <span className="muted">no longer kept</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
