// Bin analysis: how full every bin is, which bins could be emptied and which
// cards are spread over several bins. Works on the file of the last
// inventory import on the server (only the export has every pile with its
// own location). Users with "Bin moves" turn a plan into a move list.

import { Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import {
  can,
  useBins,
  useConsolidationSummary,
  useCreateMoveList,
  useExportBins,
  useFragmented,
  useMe,
  type BinParams,
} from "../api/queries";
import type { BinSort, MoveListRequest, PlanSummary } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount } from "../format";
import { useDebounced } from "../useDebounced";
import { parseWhole, sourceText } from "./bins";
import { saveDownload } from "./buyHelper";

export function BinAnalysisPage() {
  const { data: me } = useMe();
  const [freeText, setFreeText] = useState("5");
  const [sort, setSort] = useState<BinSort>("free");
  const minFree = useDebounced(parseWhole(freeText, 0, 60), 300);
  const params: BinParams | null = minFree === null ? null : { minFree, sort };
  const bins = useBins(params);
  const exporting = useExportBins();
  const source = bins.data?.source;

  return (
    <section className="page page--wide">
      <h1>Bin analysis</h1>
      {bins.data && !source && (
        <p className="notice notice--warning">
          Bin analysis needs an inventory import on the server: only the Cardmarket export lists every pile with its own
          location.{" "}
          {can(me, "inventory.import") ? (
            <Link to="/inventory-import">Import the inventory</Link>
          ) : (
            "Ask someone who may import the inventory."
          )}
        </p>
      )}
      {source && <p className="data-as-of">{sourceText(source)}</p>}
      <ErrorMessage error={bins.error} />

      <h2>Bin capacity</h2>
      <div className="panel precon-controls">
        <span className="inline-field">
          <label htmlFor="bins-free">Min free slots</label>
          <input
            id="bins-free"
            className="number-field__input"
            value={freeText}
            inputMode="numeric"
            maxLength={2}
            aria-invalid={parseWhole(freeText, 0, 60) === null}
            onChange={(e) => setFreeText(e.target.value)}
          />
        </span>
        <span className="inline-field">
          <label htmlFor="bins-sort">Sort by</label>
          <select id="bins-sort" value={sort} onChange={(e) => setSort(e.target.value as BinSort)}>
            <option value="free">Free slots</option>
            <option value="location">Location</option>
          </select>
        </span>
        <button
          type="button"
          className="button"
          disabled={!params || !bins.data?.rows.length || exporting.isPending}
          onClick={() => params && exporting.mutate(params, { onSuccess: saveDownload })}
        >
          Download CSV
        </button>
      </div>
      {bins.data && source && (
        <>
          <p className="muted">
            {formatCount(bins.data.bins_total)} bins hold {formatCount(bins.data.cards_total)} cards (a bin holds{" "}
            {bins.data.capacity}).{" "}
            {bins.data.unplaced_rows > 0 &&
              `${formatCount(bins.data.unplaced_rows)} rows in stock have no bin location. `}
            {formatCount(bins.data.rows.length)} bins have at least {minFree} free slots.
          </p>
          {bins.data.rows.length > 0 && (
            <div className="table-wrap bins-table">
              <table className="table" aria-label="Bins">
                <thead>
                  <tr>
                    <th scope="col">Bin</th>
                    <th scope="col" className="num">
                      Cards
                    </th>
                    <th scope="col" className="num">
                      Piles
                    </th>
                    <th scope="col" className="num">
                      Free
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {bins.data.rows.map((r) => (
                    <tr key={r.bin}>
                      <th scope="row" className="nowrap">
                        {r.bin}
                      </th>
                      <td className="num">{r.cards}</td>
                      <td className="num">{r.piles}</td>
                      <td className={r.free < 0 ? "num change--down" : "num"}>{r.free}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}

      {source && <Consolidation canMove={can(me, "bins.use")} />}
      {source && <Fragmented canMove={can(me, "bins.use")} />}
    </section>
  );
}

function MakeList({ request, summary }: { request: MoveListRequest; summary: PlanSummary }) {
  const create = useCreateMoveList();
  const navigate = useNavigate();
  return (
    <div className="import-actions">
      <button
        type="button"
        className="button button--primary"
        disabled={summary.moves === 0 || summary.too_large || create.isPending}
        onClick={() => create.mutate(request, { onSuccess: () => void navigate({ to: "/bin-moves" }) })}
      >
        Make move list
      </button>
      <span className="muted small">
        Replaces your current move list; ticked piles that are in the new one stay ticked.
      </span>
      <ErrorMessage error={create.error} />
    </div>
  );
}

function PlanText({ summary }: { summary: PlanSummary }) {
  if (summary.moves === 0) return <p className="muted">Nothing to move.</p>;
  const freed = summary.bins_freed.length;
  return (
    <>
      <p>
        {formatCount(summary.cards_moved)} {summary.cards_moved === 1 ? "card" : "cards"} in{" "}
        {formatCount(summary.moves)} {summary.moves === 1 ? "move" : "moves"};{" "}
        {freed === 1 ? "1 bin ends" : `${formatCount(freed)} bins end`} up empty. Walking distance{" "}
        {formatCount(summary.total_distance)} (lower is better).
      </p>
      {freed > 0 && <p className="muted small">Emptied: {summary.bins_freed.join(", ")}</p>}
      {summary.too_large && (
        <p className="notice notice--warning">That is more than 5,000 moves, too many for one list.</p>
      )}
    </>
  );
}

function Consolidation({ canMove }: { canMove: boolean }) {
  const [text, setText] = useState("20");
  const threshold = useDebounced(parseWhole(text, 1, 60), 300);
  const summary = useConsolidationSummary(threshold);
  return (
    <>
      <h2>Empty sparse bins</h2>
      <p className="muted">
        Moves the piles of bins filled up to the limit into fuller bins, preferring bins that already hold the same card
        and short ways. Each card keeps its lot and side; only the bin changes.
      </p>
      <div className="panel">
        <span className="inline-field">
          <label htmlFor="bins-threshold">Empty bins with at most</label>
          <input
            id="bins-threshold"
            className="number-field__input"
            value={text}
            inputMode="numeric"
            maxLength={2}
            aria-invalid={parseWhole(text, 1, 60) === null}
            onChange={(e) => setText(e.target.value)}
          />
          <span>cards</span>
        </span>
        {summary.data && (
          <>
            <p className="muted small">
              {summary.data.sources_considered === 1
                ? "1 bin is that sparse."
                : `${formatCount(summary.data.sources_considered)} bins are that sparse.`}
            </p>
            <PlanText summary={summary.data} />
            {canMove && threshold !== null && (
              <MakeList request={{ kind: "consolidation", threshold }} summary={summary.data} />
            )}
          </>
        )}
        <ErrorMessage error={summary.error} />
      </div>
    </>
  );
}

function Fragmented({ canMove }: { canMove: boolean }) {
  const fragmented = useFragmented();
  const data = fragmented.data;
  return (
    <>
      <h2>Cards spread over several bins</h2>
      <p className="muted">
        The same card (condition, language, foil) in more than one bin, regardless of how full the bins are. Gathering
        moves each into the bin it already occupies that is closest for the others and has room.
      </p>
      <ErrorMessage error={fragmented.error} />
      {data?.defrag && (
        <div className="panel">
          <p>{formatCount(data.total)} cards are spread over several bins.</p>
          <PlanText summary={data.defrag} />
          {canMove && <MakeList request={{ kind: "defrag" }} summary={data.defrag} />}
        </div>
      )}
      {data && data.variants.length > 0 && (
        <div className="table-wrap">
          <table className="table" aria-label="Spread cards">
            <thead>
              <tr>
                <th scope="col">Card</th>
                <th scope="col" className="num">
                  Copies
                </th>
                <th scope="col">Piles</th>
              </tr>
            </thead>
            <tbody>
              {data.variants.map((v, i) => (
                <tr key={`${i}-${v.name}`}>
                  <td>
                    {v.name}
                    {v.foil && " ✦"}
                    <span className="muted small">
                      {" "}
                      {v.condition} · {v.language}
                    </span>
                  </td>
                  <td className="num">×{v.copies}</td>
                  <td>{v.piles.map((p) => `${p.location} ×${p.quantity}`).join(", ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {data.total > data.variants.length && (
            <p className="muted small">The first {data.variants.length} are shown.</p>
          )}
        </div>
      )}
    </>
  );
}
