// Bulk lots: bundle old, cheap singles into one Cardmarket lot. Choose the
// rules (with a live preview), pick the cards from the shelves (on the
// phone; "Replace" swaps a card you cannot find), finish the lot (its price
// is booked as revenue, the cards are reserved until the next import, and
// the TCG Powertools CSV downloads), and manage recorded lots.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  can,
  downloadBulkCsv,
  useBulkDraft,
  useBulkHistory,
  useBulkLotCards,
  useBulkOptions,
  useBulkPicked,
  useBulkPreview,
  useBulkTypes,
  useDropBulkDraft,
  useFinishBulkLot,
  useLoadBulkTypes,
  useMakeBulkDraft,
  useManageBulkLot,
  useMe,
  useReplaceBulkPile,
} from "../api/queries";
import type { BulkLotView, BulkOptions, BulkPreview, DraftView, LotLine, SpecView } from "../api/types";
import { Chip, Field, RuleGroup, Stat } from "../components/Controls";
import { ErrorMessage } from "../components/ErrorMessage";
import { JobProgress } from "../components/JobProgress";
import { Modal } from "../components/Modal";
import { formatCount, formatEuro } from "../format";
import { markSeen } from "../inventoryVersion";
import { useDebounced } from "../useDebounced";
import { sourceText } from "./bins";
import { saveDownload } from "./buyHelper";
import {
  CARD_TYPES,
  daysText,
  FOILS,
  ORDERS,
  parsePrice,
  pickedTotals,
  RARITIES,
  STATUS_LABELS,
  takenLines,
} from "./bulkLots";
import { typedCount } from "./inventoryImport";

type Tab = "choose" | "pick" | "history";

export function BulkLotsPage() {
  const options = useBulkOptions();
  const draft = useBulkDraft();
  const [tab, setTab] = useState<Tab>("choose");
  const hasDraft = Boolean(draft.data);

  return (
    <section className="page page--wide">
      <h1>Bulk lots</h1>
      <div className="tabs" role="tablist" aria-label="Steps">
        {(
          [
            ["choose", "1. Choose cards"],
            ["pick", hasDraft ? `2. Pick (${draft.data?.lines.length ?? 0})` : "2. Pick"],
            ["history", "History"],
          ] as [Tab, string][]
        ).map(([key, label]) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={tab === key}
            className={tab === key ? "tab tab--active" : "tab"}
            disabled={key === "pick" && !hasDraft}
            onClick={() => setTab(key)}
          >
            {label}
          </button>
        ))}
      </div>
      <ErrorMessage error={options.error ?? draft.error} />
      {options.data && !options.data.source && (
        <p className="notice notice--warning">
          Bulk lots need an inventory import on the server (Tools → Inventory import).
        </p>
      )}
      {tab === "choose" && options.data?.source && (
        <Choose options={options.data} hasDraft={hasDraft} onStart={() => setTab("pick")} />
      )}
      {tab === "pick" && draft.data && (
        <Pick draft={draft.data} options={options.data} onFinished={() => setTab("history")} />
      )}
      {tab === "history" && <History />}
    </section>
  );
}

// ── 1. Choose ──────────────────────────────────────────────────────────────

/** A number input with a unit after it. */
function NumberInput({
  id,
  value,
  onChange,
  unit,
  step,
}: {
  id: string;
  value: number;
  onChange: (value: number) => void;
  unit?: string;
  step?: string;
}) {
  return (
    <span className="unit-input">
      <input
        id={id}
        type="number"
        min={0}
        step={step ?? "1"}
        value={Number.isFinite(value) ? value : ""}
        onChange={(e) => onChange(e.target.value === "" ? Number.NaN : Number(e.target.value))}
      />
      {unit && <span className="unit-input__unit">{unit}</span>}
    </span>
  );
}

/** Cards left after each rule, as bars; the tightest rule is marked. */
function Funnel({ preview: p }: { preview: BulkPreview }) {
  const base = Math.max(1, p.funnel.steps[0]?.copies ?? 1);
  return (
    <section className="panel funnel" aria-label="How the cards were chosen">
      <h3 className="rule-group__title">How the cards were chosen</h3>
      <p className="muted small">Cards left after each rule, in order:</p>
      <ol className="funnel__steps">
        {p.funnel.steps.map((s) => {
          const tight = s.label === p.funnel.bottleneck;
          const width = Math.max(0, Math.min(100, (s.copies / base) * 100));
          return (
            <li key={s.label} className={tight ? "funnel__step funnel__step--tight" : "funnel__step"}>
              <span className="funnel__label">
                {s.label}
                {tight && <span className="funnel__badge">tightest rule</span>}
              </span>
              <span className="funnel__count">{formatCount(s.copies)}</span>
              <svg className="funnel__bar" viewBox="0 0 100 8" preserveAspectRatio="none" aria-hidden="true">
                <rect className="funnel__track" width="100" height="8" rx="2" />
                <rect className="funnel__fill" width={width} height="8" rx="2" />
              </svg>
            </li>
          );
        })}
      </ol>
      {p.funnel.reserved > 0 && (
        <p className="muted small">
          {formatCount(p.funnel.reserved)} copies are reserved for lots awaiting their import.
        </p>
      )}
    </section>
  );
}

function Choose({ options, hasDraft, onStart }: { options: BulkOptions; hasDraft: boolean; onStart: () => void }) {
  const [spec, setSpec] = useState<SpecView>(options.default_spec);
  const debounced = useDebounced(spec, 400);
  const preview = useBulkPreview(debounced);
  const types = useBulkTypes();
  const loadTypes = useLoadBulkTypes();
  const make = useMakeBulkDraft();
  const queryClient = useQueryClient();
  const update = (change: Partial<SpecView>) => setSpec((s) => ({ ...s, ...change }));
  const toggle = <T,>(list: T[], value: T) =>
    list.includes(value) ? list.filter((v) => v !== value) : [...list, value];
  const p = preview.data;
  const running = types.data?.state === "running";

  // When a type lookup ends, the preview has new cards to show.
  const lastState = types.data?.state;
  useEffect(() => {
    if (lastState === "finished") void queryClient.invalidateQueries({ queryKey: ["bulk-preview"] });
  }, [lastState, queryClient]);

  return (
    <>
      {options.source && <p className="data-as-of">{sourceText(options.source)}</p>}
      <div className="bulk-layout">
        <section className="panel bulk-rules" aria-label="Rules">
          <RuleGroup title="Start from">
            <div className="chip-row">
              {options.presets.map((preset) => (
                <button
                  key={preset.label}
                  type="button"
                  className="button button--small"
                  onClick={() =>
                    setSpec((s) => ({
                      ...preset.spec,
                      excluded_conditions: s.excluded_conditions,
                      excluded_languages: s.excluded_languages,
                      foil: s.foil,
                      min_age_days: s.min_age_days,
                      order: s.order,
                      seed: s.seed,
                    }))
                  }
                >
                  {preset.label}
                </button>
              ))}
            </div>
          </RuleGroup>

          <RuleGroup title="Lot">
            <Field label="Cards in the lot" htmlFor="bulk-size">
              <NumberInput id="bulk-size" value={spec.size} onChange={(v) => update({ size: v })} unit="cards" />
            </Field>
          </RuleGroup>

          <RuleGroup title="Cards">
            <Field label="Rarity">
              <div className="chip-row">
                {RARITIES.map((r) => (
                  <Chip
                    key={r.key}
                    on={spec.rarities.includes(r.key)}
                    onClick={() => update({ rarities: toggle(spec.rarities, r.key) })}
                    title={r.key === "other" ? "Tokens, basic lands, specials and other rarities" : undefined}
                  >
                    {r.key === "other" ? "Other" : r.label}
                  </Chip>
                ))}
              </div>
            </Field>
            <Field label="Card type" htmlFor="bulk-type">
              <select
                id="bulk-type"
                className="rule-select"
                value={spec.card_type}
                onChange={(e) => update({ card_type: e.target.value as SpecView["card_type"] })}
              >
                {CARD_TYPES.map((t) => (
                  <option key={t.key} value={t.key}>
                    {t.label}
                  </option>
                ))}
              </select>
            </Field>
            <label className="switch">
              <input
                type="checkbox"
                checked={spec.exclude_basic_lands}
                onChange={(e) => update({ exclude_basic_lands: e.target.checked })}
              />
              <span>Leave out basic lands</span>
            </label>
            <Field label="Finish">
              <div className="segmented" role="group" aria-label="Finish">
                {FOILS.map((f) => (
                  <button
                    key={f.key}
                    type="button"
                    className={spec.foil === f.key ? "segmented__button segmented__button--on" : "segmented__button"}
                    aria-pressed={spec.foil === f.key}
                    onClick={() => update({ foil: f.key })}
                  >
                    {f.label}
                  </button>
                ))}
              </div>
            </Field>
            <Field label="Condition" hint="Highlighted conditions may go into the lot.">
              <div className="chip-row">
                {options.conditions.map((c) => (
                  <Chip
                    key={c}
                    on={!spec.excluded_conditions.includes(c)}
                    onClick={() => update({ excluded_conditions: toggle(spec.excluded_conditions, c) })}
                  >
                    {c}
                  </Chip>
                ))}
              </div>
            </Field>
            {options.languages.length > 1 && (
              <Field label="Language">
                <div className="chip-row">
                  {options.languages.map((l) => (
                    <Chip
                      key={l.language}
                      on={!spec.excluded_languages.includes(l.language)}
                      onClick={() => update({ excluded_languages: toggle(spec.excluded_languages, l.language) })}
                      title={`${formatCount(l.copies)} in stock`}
                    >
                      {l.language} <span className="chip__count">{formatCount(l.copies)}</span>
                    </Chip>
                  ))}
                </div>
              </Field>
            )}
          </RuleGroup>

          <RuleGroup title="Value">
            <div className="rule-row">
              <Field label="Max per card" htmlFor="bulk-max-price">
                <NumberInput
                  id="bulk-max-price"
                  step="0.01"
                  value={spec.max_price}
                  onChange={(v) => update({ max_price: v })}
                  unit="€"
                />
              </Field>
              <Field label="Guaranteed hits" htmlFor="bulk-hits">
                <NumberInput id="bulk-hits" value={spec.hits} onChange={(v) => update({ hits: v })} unit="cards" />
              </Field>
              <Field label="Hit worth at least" htmlFor="bulk-hit-price">
                <NumberInput
                  id="bulk-hit-price"
                  step="0.01"
                  value={spec.hit_min_price}
                  onChange={(v) => update({ hit_min_price: v })}
                  unit="€"
                />
              </Field>
            </div>
          </RuleGroup>

          <RuleGroup title="Selection">
            <Field label="Order" htmlFor="bulk-order">
              <select
                id="bulk-order"
                className="rule-select"
                value={spec.order}
                onChange={(e) => update({ order: e.target.value as SpecView["order"] })}
              >
                {ORDERS.map((o) => (
                  <option key={o.key} value={o.key}>
                    {o.label}
                  </option>
                ))}
              </select>
            </Field>
            <div className="rule-row">
              <Field label="In stock at least" htmlFor="bulk-age" hint="Counted from the first import with the card.">
                <NumberInput
                  id="bulk-age"
                  value={spec.min_age_days}
                  onChange={(v) => update({ min_age_days: v })}
                  unit="days"
                />
              </Field>
              <Field label="Copies per name" htmlFor="bulk-per-name" hint="1 = no repeated card names.">
                <NumberInput
                  id="bulk-per-name"
                  value={spec.max_copies_per_name}
                  onChange={(v) => update({ max_copies_per_name: v })}
                />
              </Field>
            </div>
            <button
              type="button"
              className="button button--small"
              onClick={() => update({ seed: (spec.seed % 1_000_000) + 1 })}
            >
              Shuffle again
            </button>
          </RuleGroup>
        </section>

        <div className="bulk-result">
          <ErrorMessage error={preview.error} />
          {p && (
            <>
              <div className="stats" role="group" aria-label="The lot" aria-busy={preview.isFetching}>
                <Stat
                  label="Cards"
                  value={`${formatCount(p.summary.copies)} / ${formatCount(p.size)}`}
                  sub={`${formatCount(p.summary.distinct_names)} different names`}
                  tone={p.summary.copies >= p.size ? "good" : "warning"}
                />
                <Stat
                  label="Single value"
                  value={formatEuro(p.summary.listed_value)}
                  sub="listed price of these cards"
                />
                <Stat
                  label="In stock for"
                  value={daysText(p.summary.oldest_days)}
                  sub={
                    p.summary.newest_days !== null && p.summary.newest_days !== p.summary.oldest_days
                      ? `newest: ${daysText(p.summary.newest_days)}`
                      : "all the same age"
                  }
                />
                {spec.hits > 0 && (
                  <Stat
                    label="Hits"
                    value={formatCount(p.summary.hits)}
                    sub={`worth ≥ ${formatEuro(spec.hit_min_price)}`}
                    tone={p.summary.hits >= spec.hits ? "good" : "warning"}
                  />
                )}
              </div>
              {preview.isFetching && <p className="muted small">Updating…</p>}

              {p.types_needed > 0 && (
                <div className="notice notice--warning">
                  <p>
                    <strong>Card types needed.</strong> {formatCount(p.types_needed)}{" "}
                    {p.types_needed === 1 ? "card needs its" : "cards need their"} card type from Scryfall before they
                    can count for “{CARD_TYPES.find((t) => t.key === spec.card_type)?.label}”. Looked up once, then
                    cached.
                  </p>
                  <div className="import-actions">
                    <button
                      type="button"
                      className="button"
                      disabled={running || loadTypes.isPending}
                      onClick={() => loadTypes.mutate(spec)}
                    >
                      Look up card types
                    </button>
                    {types.data && <JobProgress status={types.data} />}
                  </div>
                  {types.data?.state === "failed" && <p className="error">{types.data.error}</p>}
                  <ErrorMessage error={loadTypes.error} />
                </div>
              )}
              {p.summary.copies < p.size && (
                <div className="notice notice--warning">
                  <p>
                    <strong>
                      Only {formatCount(p.summary.copies)} of {formatCount(p.size)} match.
                    </strong>{" "}
                    {p.funnel.bottleneck
                      ? `The tightest rule is “${p.funnel.bottleneck}”. Loosen it, allow more copies per name, or make the lot smaller.`
                      : "Allow more copies per name or make the lot smaller."}
                  </p>
                </div>
              )}

              <div className="bulk-start">
                <button
                  type="button"
                  className="button button--primary"
                  disabled={p.summary.copies === 0 || make.isPending || preview.isFetching}
                  onClick={() => make.mutate(spec, { onSuccess: onStart })}
                >
                  {hasDraft ? "Replace the lot in progress and pick" : "Start picking"}
                </button>
                {p.summary.purchase_lots.length > 0 && (
                  <span className="muted small">
                    From purchase lots {p.summary.purchase_lots.map(([lot, n]) => `${lot || "none"} ×${n}`).join(", ")}
                  </span>
                )}
              </div>
              <ErrorMessage error={make.error} />

              <Funnel preview={p} />
              <LinesTable lines={p.lines} />
            </>
          )}
        </div>
      </div>
    </>
  );
}

function LinesTable({ lines }: { lines: LotLine[] }) {
  if (lines.length === 0) return null;
  return (
    <section aria-label="The cards">
      <h3 className="rule-group__title bulk-lines__title">The cards</h3>
      <div className="table-wrap bulk-lines">
        <table className="table" aria-label="Cards of the lot">
          <thead>
            <tr>
              <th scope="col">Card</th>
              <th scope="col">Set</th>
              <th scope="col">Cond.</th>
              <th scope="col" className="num">
                Copies
              </th>
              <th scope="col" className="num">
                Price
              </th>
              <th scope="col">In stock</th>
              <th scope="col">Location</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((l) => (
              <tr key={l.pile}>
                <td>
                  {l.name}
                  {l.foil && " ✦"}
                  {l.hit && <span className="muted small"> · hit</span>}
                </td>
                <td className="nowrap">{l.set_code}</td>
                <td className="nowrap">
                  {l.condition} · {l.language}
                </td>
                <td className="num">×{l.copies}</td>
                <td className="num">{formatEuro(l.price)}</td>
                <td className="nowrap">{daysText(l.age_days)}</td>
                <td className="nowrap">{l.location || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

// ── 2. Pick + finish ───────────────────────────────────────────────────────

function Pick({ draft, options, onFinished }: { draft: DraftView; options?: BulkOptions; onFinished: () => void }) {
  const { data: me } = useMe();
  const picked = useBulkPicked();
  const replace = useReplaceBulkPile();
  const drop = useDropBulkDraft();
  const [finishing, setFinishing] = useState(false);
  const [confirmDrop, setConfirmDrop] = useState(false);
  const [showPicked, setShowPicked] = useState(true);
  const t = pickedTotals(draft.lines);
  const taken = takenLines(draft.lines);
  const shown = draft.lines.filter((l) => showPicked || !l.picked);

  return (
    <>
      {draft.gone > 0 && (
        <p className="notice notice--warning">
          {draft.gone} {draft.gone === 1 ? "card is" : "cards are"} no longer in the stock (a new import) and left out.
        </p>
      )}
      {taken.length > 0 && (
        <p className="notice notice--warning" role="alert">
          {taken.length} {taken.length === 1 ? "card was" : "cards were"} taken by another lot or a discard meanwhile:
          replace {taken.length === 1 ? "it" : "them"} before finishing.
        </p>
      )}
      <div className="panel move-controls">
        <div className="move-progress">
          <span>
            Picked {t.lines} of {draft.lines.length}
          </span>
          <progress max={draft.lines.length} value={t.lines} aria-label="Picked cards" />
          <span className="muted">{formatCount(t.copies)} copies</span>
        </div>
        <label className="checkbox">
          <input type="checkbox" checked={showPicked} onChange={(e) => setShowPicked(e.target.checked)} />
          Show picked cards
        </label>
        <div className="move-controls__buttons">
          <button
            type="button"
            className="button"
            disabled={picked.isPending || t.lines === draft.lines.length}
            onClick={() =>
              picked.mutate({ piles: draft.lines.filter((l) => !l.picked).map((l) => l.pile), picked: true })
            }
          >
            Mark all picked
          </button>
          {can(me, "bulk_lot.finish") && (
            <button
              type="button"
              className="button button--primary"
              disabled={t.lines === 0}
              onClick={() => setFinishing(true)}
            >
              Finish the lot…
            </button>
          )}
          <button type="button" className="button button--ghost" onClick={() => setConfirmDrop(true)}>
            Discard this lot
          </button>
        </div>
      </div>
      <ErrorMessage error={picked.error ?? replace.error ?? drop.error} />
      <div className="move-tiles">
        {shown.map((l) => (
          <PickTile
            key={l.pile}
            line={l}
            busy={replace.isPending}
            onToggle={() => picked.mutate({ piles: [l.pile], picked: !l.picked })}
            onReplace={() => replace.mutate(l.pile)}
          />
        ))}
      </div>
      {finishing && (
        <FinishDialog
          draft={draft}
          maxPrice={options?.max_lot_price ?? 100_000}
          onClose={() => setFinishing(false)}
          onFinished={() => {
            setFinishing(false);
            onFinished();
          }}
        />
      )}
      {confirmDrop && (
        <Modal title="Discard this lot?" onClose={() => setConfirmDrop(false)}>
          <p>The chosen cards and their ticks are removed. Nothing is recorded and the stock is not changed.</p>
          <div className="import-actions">
            <button
              type="button"
              className="button button--danger"
              onClick={() => {
                setConfirmDrop(false);
                drop.mutate();
              }}
            >
              Discard
            </button>
            <button type="button" className="button button--ghost" onClick={() => setConfirmDrop(false)}>
              Keep it
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}

function PickTile({
  line: l,
  busy,
  onToggle,
  onReplace,
}: {
  line: LotLine;
  busy: boolean;
  onToggle: () => void;
  onReplace: () => void;
}) {
  const [imageFailed, setImageFailed] = useState(false);
  const isTaken = l.free < l.copies;
  return (
    <article className={l.picked ? "move-tile move-tile--done" : "move-tile"} aria-label={l.name}>
      {l.image && !imageFailed ? (
        <img className="move-tile__image" src={l.image} alt="" loading="lazy" onError={() => setImageFailed(true)} />
      ) : (
        <div className="move-tile__image move-tile__image--none" aria-hidden="true">
          {l.set_code}
        </div>
      )}
      <div className="move-tile__body">
        <p className="move-tile__name">
          {l.name}
          {l.foil && " ✦"}
        </p>
        <p className="muted small">
          {l.set_code}
          {l.collector_number && ` #${l.collector_number}`} · {l.condition} · {l.language}
        </p>
        <p className="move-tile__route">
          <strong>×{l.copies}</strong> {l.location || "no location"}
        </p>
        {isTaken && <p className="change--down small">Taken meanwhile ({l.free} free)</p>}
        <div className="move-controls__buttons">
          <button
            type="button"
            className={l.picked ? "button move-tile__toggle" : "button button--primary move-tile__toggle"}
            aria-pressed={l.picked}
            onClick={onToggle}
          >
            {l.picked ? "Picked ✓" : "Pick"}
          </button>
          <button type="button" className="button button--ghost button--small" disabled={busy} onClick={onReplace}>
            Replace
          </button>
        </div>
      </div>
    </article>
  );
}

function FinishDialog({
  draft,
  maxPrice,
  onClose,
  onFinished,
}: {
  draft: DraftView;
  maxPrice: number;
  onClose: () => void;
  onFinished: () => void;
}) {
  const finish = useFinishBulkLot();
  const queryClient = useQueryClient();
  const [name, setName] = useState(draft.default_name);
  const [priceText, setPriceText] = useState("");
  const price = parsePrice(priceText);
  const t = pickedTotals(draft.lines);
  const valid = name.trim().length > 0 && price !== null && price <= maxPrice;

  return (
    <Modal title="Finish the lot" onClose={onClose}>
      <form
        className="form"
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid || price === null) return;
          finish.mutate(
            { name: name.trim(), price },
            {
              onSuccess: (result) => {
                markSeen(result.data_version);
                void queryClient.invalidateQueries();
                void downloadBulkCsv(result.id).then(saveDownload);
                onFinished();
              },
            },
          );
        }}
      >
        <label className="field">
          <span>Name</span>
          <input value={name} maxLength={120} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span>Price the lot sells for (€)</span>
          <input
            inputMode="decimal"
            value={priceText}
            aria-invalid={priceText !== "" && price === null}
            onChange={(e) => setPriceText(e.target.value)}
          />
        </label>
        <p>
          {formatCount(t.copies)} picked copies in {t.lines} lines, listed at {formatEuro(t.listedValue)}
          {price !== null && t.copies > 0 && ` · ${formatEuro(price / t.copies)} per card`}.
        </p>
        <p className="muted small">
          Only the cards marked as picked go into the lot. Finishing books the price as today&apos;s revenue, reserves
          the cards until the next import takes them out, takes a backup, and downloads the TCG Powertools CSV: import
          it in Powertools to remove the cards from Cardmarket.
        </p>
        <ErrorMessage error={finish.error} />
        <div className="import-actions">
          <button type="submit" className="button button--primary" disabled={!valid || finish.isPending}>
            {finish.isPending ? "Recording…" : "Record the lot and download the CSV"}
          </button>
          <button type="button" className="button button--ghost" onClick={onClose}>
            Cancel
          </button>
        </div>
      </form>
    </Modal>
  );
}

// ── History ────────────────────────────────────────────────────────────────

function History() {
  const { data: me } = useMe();
  const history = useBulkHistory();
  const mayManage = can(me, "bulk_lot.manage");
  const mayDownload = can(me, "bulk_lot.finish");
  const [cardsOf, setCardsOf] = useState<number | null>(null);
  const [dialog, setDialog] = useState<{ lot: BulkLotView; kind: "price" | "removed" | "undo" } | null>(null);

  if (!history.data) return <ErrorMessage error={history.error} />;
  if (history.data.length === 0) return <p className="muted">No bulk lots yet.</p>;
  return (
    <>
      <div className="table-wrap">
        <table className="table" aria-label="Bulk lots">
          <thead>
            <tr>
              <th scope="col">Day</th>
              <th scope="col">Name</th>
              <th scope="col" className="num">
                Copies
              </th>
              <th scope="col">Status</th>
              {history.data.some((l) => l.price !== null) && (
                <th scope="col" className="num">
                  Price
                </th>
              )}
              <th scope="col">
                <span className="visually-hidden">Actions</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {history.data.map((l) => (
              <tr key={l.id}>
                <td className="nowrap">{l.created_at}</td>
                <td>{l.name}</td>
                <td className="num">
                  ×{l.copies}
                  {l.pending_copies > 0 && <span className="muted small"> ({l.pending_copies} pending)</span>}
                </td>
                <td className={l.status === "overdue" ? "change--down" : undefined}>{STATUS_LABELS[l.status]}</td>
                {l.price !== null && <td className="num">{formatEuro(l.price)}</td>}
                <td className="search-table__actions">
                  <button type="button" className="button button--small" onClick={() => setCardsOf(l.id)}>
                    Cards
                  </button>
                  {mayDownload && l.csv_available && (
                    <button
                      type="button"
                      className="button button--small"
                      onClick={() => void downloadBulkCsv(l.id).then(saveDownload)}
                    >
                      CSV
                    </button>
                  )}
                  {mayManage && (
                    <>
                      <button
                        type="button"
                        className="button button--small"
                        onClick={() => setDialog({ lot: l, kind: "price" })}
                      >
                        Price…
                      </button>
                      {l.pending_copies > 0 && (
                        <button
                          type="button"
                          className="button button--small"
                          onClick={() => setDialog({ lot: l, kind: "removed" })}
                        >
                          Mark removed…
                        </button>
                      )}
                      <button
                        type="button"
                        className="button button--small button--danger"
                        onClick={() => setDialog({ lot: l, kind: "undo" })}
                      >
                        Undo…
                      </button>
                    </>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {cardsOf !== null && <LotCards id={cardsOf} onClose={() => setCardsOf(null)} />}
      {dialog && <ManageDialog lot={dialog.lot} kind={dialog.kind} onClose={() => setDialog(null)} />}
    </>
  );
}

function LotCards({ id, onClose }: { id: number; onClose: () => void }) {
  const cards = useBulkLotCards(id);
  return (
    <Modal title={`Cards of lot ${id}`} onClose={onClose}>
      <ErrorMessage error={cards.error} />
      <ul className="selection-list">
        {cards.data?.map((c, i) => (
          <li key={`${i}-${c.name}`}>
            <span>
              ×{c.copies} {c.name} ({c.set_name}, {c.condition}, {c.language}) · {c.location || "—"}
              {c.pending_copies > 0 && <span className="muted small"> · {c.pending_copies} pending</span>}
            </span>
          </li>
        ))}
      </ul>
    </Modal>
  );
}

function ManageDialog({
  lot,
  kind,
  onClose,
}: {
  lot: BulkLotView;
  kind: "price" | "removed" | "undo";
  onClose: () => void;
}) {
  const manage = useManageBulkLot();
  const [text, setText] = useState(kind === "price" && lot.price !== null ? lot.price.toFixed(2) : "");
  const removed = lot.copies - lot.pending_copies;
  const done = (result: { data_version: number }) => {
    markSeen(result.data_version);
    onClose();
  };
  const titles = { price: "Correct the price", removed: "Mark the removal as done", undo: "Undo the lot" };
  const price = parsePrice(text);
  const typedRight = removed === 0 || typedCount(text) === removed;

  return (
    <Modal title={`${titles[kind]} — ${lot.name}`} onClose={onClose}>
      {kind === "price" && (
        <>
          <label className="field">
            <span>Price it sold for (€)</span>
            <input inputMode="decimal" value={text} onChange={(e) => setText(e.target.value)} />
          </label>
          <p className="muted small">The revenue of its cards follows the new price.</p>
        </>
      )}
      {kind === "removed" && (
        <p>
          Declares the {lot.pending_copies} pending copies gone without waiting for an import. Only do this when they
          are already out of your inventory export: if they are still in it, their later disappearance counts as sales.
          A backup is taken first.
        </p>
      )}
      {kind === "undo" && (
        <>
          <p>Deletes the lot with its revenue. Copies still waiting for an import count as ordinary stock again.</p>
          {removed > 0 && (
            <>
              <p className="notice notice--danger">
                An import already took {removed} copies of this lot out. They come back as stock, and when they
                disappear again they count as sold. Type {removed} to undo anyway.
              </p>
              <label className="field">
                <span>Copies already taken out</span>
                <input inputMode="numeric" value={text} onChange={(e) => setText(e.target.value)} />
              </label>
            </>
          )}
          <p className="muted small">A backup is taken first.</p>
        </>
      )}
      <ErrorMessage error={manage.error} />
      <div className="import-actions">
        <button
          type="button"
          className={kind === "price" ? "button button--primary" : "button button--danger"}
          disabled={manage.isPending || (kind === "price" ? price === null : kind === "undo" && !typedRight)}
          onClick={() => {
            if (kind === "price" && price !== null && lot.price !== null)
              manage.mutate({ id: lot.id, kind, price, expected: lot.price }, { onSuccess: done });
            if (kind === "removed") manage.mutate({ id: lot.id, kind }, { onSuccess: done });
            if (kind === "undo")
              manage.mutate({ id: lot.id, kind, ...(removed > 0 ? { confirm: text } : {}) }, { onSuccess: done });
          }}
        >
          {kind === "price" ? "Save" : kind === "removed" ? "Mark as removed" : "Undo the lot"}
        </button>
        <button type="button" className="button button--ghost" onClick={onClose}>
          Cancel
        </button>
      </div>
    </Modal>
  );
}
