// The user's picking list: the cards of an order in shelf order, ticked as
// they are taken (on the phone, while the list was made on the PC). It
// never changes the stock.

import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useDiscardPicking, usePicking, useSetPicked } from "../api/queries";
import type { PickItemView, PickingView } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { Modal } from "../components/Modal";
import { formatEuro } from "../format";
import { pickingTotals } from "./stockChecker";

export function PickingPage() {
  const list = usePicking();
  return (
    <section className="page page--wide">
      <h1>Picking</h1>
      <ErrorMessage error={list.error} />
      {list.isPending && <p className="loading">Loading…</p>}
      {list.data === null && (
        <p className="muted">
          No picking list. Make one in the <Link to="/stock-checker">Stock Checker</Link>.
        </p>
      )}
      {list.data && <Picking list={list.data} />}
    </section>
  );
}

function Picking({ list }: { list: PickingView }) {
  const setPicked = useSetPicked();
  const discard = useDiscardPicking();
  const [showPicked, setShowPicked] = useState(true);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const t = pickingTotals(list.items);
  const shown = list.items.filter((i) => showPicked || !i.picked);

  return (
    <>
      {list.outdated && (
        <p className="notice notice--warning">
          The stock changed since this list was made (an import or a discard): locations may have moved.
        </p>
      )}
      <div className="panel move-controls">
        <div className="move-progress">
          <span>
            Picked {t.picked} of {t.total}
          </span>
          <progress max={t.total} value={t.picked} aria-label="Picked cards" />
          <span className="muted">
            {formatEuro(t.pickedValue)} of {formatEuro(t.totalValue)}
          </span>
        </div>
        <label className="checkbox">
          <input type="checkbox" checked={showPicked} onChange={(e) => setShowPicked(e.target.checked)} />
          Show picked cards
        </label>
        <div className="move-controls__buttons">
          <button
            type="button"
            className="button"
            disabled={setPicked.isPending || t.picked === t.total}
            onClick={() =>
              setPicked.mutate({ piles: list.items.filter((i) => !i.picked).map((i) => i.pile), picked: true })
            }
          >
            Mark all picked
          </button>
          <button
            type="button"
            className="button"
            disabled={setPicked.isPending || t.picked === 0}
            onClick={() =>
              setPicked.mutate({ piles: list.items.filter((i) => i.picked).map((i) => i.pile), picked: false })
            }
          >
            Reset all
          </button>
          <button type="button" className="button button--ghost" onClick={() => setConfirmDiscard(true)}>
            Discard list
          </button>
        </div>
      </div>
      <ErrorMessage error={setPicked.error ?? discard.error} />
      <div className="move-tiles">
        {shown.map((i) => (
          <PickTile key={i.pile} item={i} onToggle={() => setPicked.mutate({ piles: [i.pile], picked: !i.picked })} />
        ))}
      </div>
      {shown.length === 0 && <p className="muted">Everything is picked.</p>}
      {confirmDiscard && (
        <Modal title="Discard the picking list?" onClose={() => setConfirmDiscard(false)}>
          <p>The list and its ticks are removed. The stock is not changed.</p>
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
    </>
  );
}

function PickTile({ item: i, onToggle }: { item: PickItemView; onToggle: () => void }) {
  const [imageFailed, setImageFailed] = useState(false);
  return (
    <article className={i.picked ? "move-tile move-tile--done" : "move-tile"} aria-label={i.name}>
      {i.image && !imageFailed ? (
        <img className="move-tile__image" src={i.image} alt="" loading="lazy" onError={() => setImageFailed(true)} />
      ) : (
        <div className="move-tile__image move-tile__image--none" aria-hidden="true">
          {i.set_code}
        </div>
      )}
      <div className="move-tile__body">
        <p className="move-tile__name">
          {i.name}
          {i.foil && " ✦"}
        </p>
        <p className="muted small">
          {i.set_code}
          {i.collector_number && ` #${i.collector_number}`} · {i.condition} · {i.language}
        </p>
        <p className="move-tile__route">
          <strong>×{i.quantity}</strong> {i.location || "no location"}
        </p>
        <p className="muted small">{formatEuro(i.price)} each</p>
        <button
          type="button"
          className={i.picked ? "button move-tile__toggle" : "button button--primary move-tile__toggle"}
          aria-pressed={i.picked}
          onClick={onToggle}
        >
          {i.picked ? "Picked ✓" : "Pick"}
        </button>
      </div>
    </article>
  );
}
