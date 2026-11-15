// The user's bin move list: piles grouped by the bin they come from, ticked
// as they are moved (on the phone while walking the shelves), and the
// ticked piles as a Cardmarket stock-update CSV. The server plans the moves
// from the last import; after a newer import the list has to be made again
// (ticks of moves still in it stay), so the CSV never carries old values.

import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useCreateMoveList, useDiscardMoveList, useExportMoves, useMoveList, useTickMoves } from "../api/queries";
import type { MoveListView, MoveView } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { Modal } from "../components/Modal";
import { formatCount } from "../format";
import { count, exportName, groupBySource, KIND_LABELS, sourceText } from "./bins";
import { saveDownload } from "./buyHelper";

export function BinMovesPage() {
  const list = useMoveList();
  return (
    <section className="page page--wide">
      <h1>Bin moves</h1>
      <ErrorMessage error={list.error} />
      {list.isPending && <p className="loading">Loading…</p>}
      {list.data === null && (
        <p className="muted">
          No move list yet. Make one on <Link to="/bins">Bin analysis</Link>.
        </p>
      )}
      {list.data && <MoveList list={list.data} />}
    </section>
  );
}

function MoveList({ list }: { list: MoveListView }) {
  const create = useCreateMoveList();
  const discard = useDiscardMoveList();
  const exporting = useExportMoves();
  const tick = useTickMoves();
  const [showDone, setShowDone] = useState(true);
  const [onlyBin, setOnlyBin] = useState("");
  const [carried, setCarried] = useState<{ carried: number; dropped: number } | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const settings =
    KIND_LABELS[list.kind] + (list.threshold !== null ? ` (bins with at most ${list.threshold} cards)` : "");

  if (list.outdated) {
    return (
      <>
        <p className="panel__title">{settings}</p>
        <div className="notice notice--warning" role="alert">
          <p>
            The inventory was imported again since this list was made, so it no longer fits the data. Make it again on
            the new import: ticked piles that are still in it stay ticked ({formatCount(list.ticked)} ticked now).
          </p>
          {list.source ? (
            <button
              type="button"
              className="button button--primary"
              disabled={create.isPending}
              onClick={() =>
                create.mutate(
                  { kind: list.kind, ...(list.threshold !== null ? { threshold: list.threshold } : {}) },
                  { onSuccess: (r) => setCarried({ carried: r.carried_ticks, dropped: r.dropped_ticks }) },
                )
              }
            >
              Make the list again
            </button>
          ) : (
            <p>There is no inventory import on the server right now.</p>
          )}
        </div>
        <ErrorMessage error={create.error} />
      </>
    );
  }

  const groups = groupBySource(list.moves);
  const shown = list.moves.filter((m) => (!onlyBin || m.from_bin === onlyBin) && (showDone || !m.done));
  const shownGroups = groupBySource(shown);
  const total = list.moves.length;
  const done = list.moves.filter((m) => m.done).length;

  return (
    <>
      <p className="panel__title">{settings}</p>
      {list.source && <p className="data-as-of">{sourceText(list.source)}</p>}
      {carried && (
        <p className="notice notice--success" role="status">
          Made again: {carried.carried} ticked piles are still in the list
          {carried.dropped > 0 && `, ${carried.dropped} are not (their move changed)`}.
        </p>
      )}
      <div className="panel move-controls">
        <div className="move-progress">
          <span>
            Moved {done} of {total}
          </span>
          <progress max={total} value={done} aria-label="Moved piles" />
        </div>
        <span className="inline-field">
          <label htmlFor="moves-bin">Bin</label>
          <select id="moves-bin" value={onlyBin} onChange={(e) => setOnlyBin(e.target.value)}>
            <option value="">All bins ({groups.length})</option>
            {groups.map((g) => (
              <option key={g.bin} value={g.bin}>
                {g.bin} · {count(g.moves.length, "pile")} · {count(g.cards, "card")}
              </option>
            ))}
          </select>
        </span>
        <label className="checkbox">
          <input type="checkbox" checked={showDone} onChange={(e) => setShowDone(e.target.checked)} />
          Show moved piles
        </label>
        <div className="move-controls__buttons">
          <button
            type="button"
            className="button"
            disabled={tick.isPending || !shown.some((m) => !m.done)}
            onClick={() => tick.mutate({ ids: shown.filter((m) => !m.done).map((m) => m.id), done: true })}
          >
            Mark shown as moved
          </button>
          <button
            type="button"
            className="button"
            disabled={tick.isPending || !shown.some((m) => m.done)}
            onClick={() => tick.mutate({ ids: shown.filter((m) => m.done).map((m) => m.id), done: false })}
          >
            Reset shown
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={done === 0 || exporting.isPending}
            title="Only the piles marked as moved"
            onClick={() => exporting.mutate(exportName(list.kind), { onSuccess: saveDownload })}
          >
            Download moved CSV ({done})
          </button>
          <button
            type="button"
            className="button button--ghost"
            disabled={discard.isPending}
            onClick={() => setConfirmDiscard(true)}
          >
            Discard list
          </button>
        </div>
        <p className="muted small">
          The CSV holds only the piles marked as moved, with their new location; import it into Cardmarket before the
          next inventory export, so the next import already knows the new places.
        </p>
      </div>
      <ErrorMessage error={tick.error ?? exporting.error ?? discard.error} />
      {confirmDiscard && (
        <Modal title="Discard the move list?" onClose={() => setConfirmDiscard(false)}>
          <p>The list and its {done} ticks are removed. The stock is not changed.</p>
          <div className="import-actions">
            <button
              type="button"
              className="button button--danger"
              onClick={() => {
                setConfirmDiscard(false);
                discard.mutate();
              }}
            >
              Discard
            </button>
            <button type="button" className="button button--ghost" onClick={() => setConfirmDiscard(false)}>
              Keep it
            </button>
          </div>
        </Modal>
      )}

      {shownGroups.map((g) => (
        <section key={g.bin} className="move-group" aria-label={`From ${g.bin}`}>
          <h2>
            From {g.bin}{" "}
            <span className="muted small">
              {count(g.moves.length, "pile")} · {count(g.cards, "card")}
            </span>
          </h2>
          <div className="move-tiles">
            {g.moves.map((m) => (
              <MoveTile key={m.id} move={m} onToggle={() => tick.mutate({ ids: [m.id], done: !m.done })} />
            ))}
          </div>
        </section>
      ))}
      {shown.length === 0 && <p className="muted">Nothing to show: every pile here is moved.</p>}
    </>
  );
}

function MoveTile({ move: m, onToggle }: { move: MoveView; onToggle: () => void }) {
  const [imageFailed, setImageFailed] = useState(false);
  return (
    <article className={m.done ? "move-tile move-tile--done" : "move-tile"} aria-label={m.name}>
      {m.image && !imageFailed ? (
        <img className="move-tile__image" src={m.image} alt="" loading="lazy" onError={() => setImageFailed(true)} />
      ) : (
        <div className="move-tile__image move-tile__image--none" aria-hidden="true">
          {m.set_code}
        </div>
      )}
      <div className="move-tile__body">
        <p className="move-tile__name">
          {m.name}
          {m.foil && " ✦"}
        </p>
        {m.local_name && <p className="muted small">{m.local_name}</p>}
        <p className="muted small">
          {m.set_code}
          {m.collector_number && ` #${m.collector_number}`} · {m.condition} · {m.language}
        </p>
        <p className="move-tile__route">
          <strong>×{m.quantity}</strong> {m.from_location} → <strong>{m.to_location}</strong>
        </p>
        <button
          type="button"
          className={m.done ? "button move-tile__toggle" : "button button--primary move-tile__toggle"}
          aria-pressed={m.done}
          onClick={onToggle}
        >
          {m.done ? "Moved ✓" : "Mark moved"}
        </button>
      </div>
    </article>
  );
}
