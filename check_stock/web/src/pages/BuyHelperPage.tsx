// Card Buy Helper: load a seller's card list, split it into singles and bulk,
// see the offer and download it as a CSV. The card list stays in the browser
// (it is sent along for every calculation, never stored); the split rules and
// rates are the user's own settings, saved on the server as they change.

import { useId, useRef, useState, type ChangeEvent, type ReactNode } from "react";
import {
  saveBuyHelperSettingsOnExit,
  useBuyHelperSettings,
  useBuyOffer,
  useExportOffer,
  useSaveBuyHelperSettings,
  type CardListFile,
} from "../api/queries";
import type { BuyHelperSettings, BuyOffer, BuyOfferParams } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount, formatEuro } from "../format";
import { useAutoSave } from "../useAutoSave";
import { useDebounced } from "../useDebounced";
import {
  fileProblem,
  formFromParams,
  paramsFromForm,
  saveDownload,
  type FormField,
  type SettingsForm,
} from "./buyHelper";

/** Wait after the last change before the offer is recalculated. */
const RECALCULATE_MS = 300;

export function BuyHelperPage() {
  const settings = useBuyHelperSettings();
  return (
    <section className="page">
      <h1>Buy helper</h1>
      <p className="muted">
        Values a seller&apos;s card list for a purchase offer. The list is only used for the calculation and never
        saved; your rules and rates are saved for your account.
      </p>
      <ErrorMessage error={settings.error} />
      {settings.isPending && <p className="loading">Loading your settings…</p>}
      {settings.data && <BuyHelper initial={settings.data} />}
    </section>
  );
}

function BuyHelper({ initial }: { initial: BuyHelperSettings }) {
  const [form, setForm] = useState<SettingsForm>(() => formFromParams(initial.params, initial.defaults));
  const [file, setFile] = useState<CardListFile | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const nextVersion = useRef(1);

  const checked = paramsFromForm(form);
  const params = checked.params ?? null;
  const offerParams = useDebounced(params, RECALCULATE_MS);
  const offer = useBuyOffer(file, offerParams);
  const exportOffer = useExportOffer();
  const saving = useSaveSettings(params, initial.params);

  async function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const chosen = input.files?.[0];
    // Choosing the same file again (after editing it) must load it again.
    input.value = "";
    if (!chosen) return;
    const problem = fileProblem(chosen);
    if (problem) {
      setFileError(problem);
      return;
    }
    try {
      const csv = await chosen.text();
      setFileError(null);
      setFile({ name: chosen.name, csv, version: nextVersion.current++ });
    } catch {
      setFileError("The file could not be read.");
    }
  }

  function update(changes: Partial<SettingsForm>) {
    setForm((current) => ({ ...current, ...changes }));
  }

  return (
    <>
      <div className="panel">
        <p className="panel__title" id="buy-helper-file-title">
          Seller&apos;s card list (CSV from the Cardmarket inventory export)
        </p>
        <div className="file-picker">
          <input
            id="buy-helper-file"
            className="visually-hidden"
            type="file"
            accept=".csv,text/csv"
            aria-labelledby="buy-helper-file-title"
            onChange={chooseFile}
          />
          <label htmlFor="buy-helper-file" className="button">
            {file ? "Choose another list…" : "Choose card list…"}
          </label>
          <span className="muted">
            {file
              ? `${file.name}${offer.data ? ` · ${formatCount(offer.data.rows)} card rows` : ""}`
              : "No list loaded"}
          </span>
        </div>
        {fileError && (
          <p className="error" role="alert">
            {fileError}
          </p>
        )}
      </div>

      <SettingsPanel
        form={form}
        errors={checked.errors ?? {}}
        onChange={update}
        onReset={() => setForm(formFromParams(initial.defaults, initial.defaults))}
        status={
          saving.error ? (
            <ErrorMessage error={saving.error} />
          ) : (
            <span className="muted" aria-live="polite">
              {saving.isPending ? "Saving…" : saving.isSuccess ? "Saved for your account" : ""}
            </span>
          )
        }
      />

      {file && (
        <div aria-live="polite">
          {offer.isFetching && !offer.data && <p className="loading">Calculating the offer…</p>}
          <ErrorMessage error={offer.error} />
          {offer.data && !offer.error && (
            <OfferView
              offer={offer.data}
              updating={offer.isFetching || JSON.stringify(offerParams) !== JSON.stringify(params)}
              exportButton={
                <button
                  type="button"
                  className="button button--primary"
                  disabled={!params || exportOffer.isPending}
                  onClick={() => {
                    if (!params) return;
                    exportOffer.mutate({ file, params }, { onSuccess: saveDownload });
                  }}
                >
                  {exportOffer.isPending ? "Preparing…" : "Download offer CSV"}
                </button>
              }
            />
          )}
          <ErrorMessage error={exportOffer.error} />
        </div>
      )}
    </>
  );
}

/** Saves the settings on the server as they change (see {@link useAutoSave}). */
function useSaveSettings(params: BuyOfferParams | null, stored: BuyOfferParams) {
  const { mutate: save, error, isPending, isSuccess } = useSaveBuyHelperSettings();
  useAutoSave(params, stored, save, saveBuyHelperSettingsOnExit);
  return { error, isPending, isSuccess };
}

function SettingsPanel({
  form,
  errors,
  onChange,
  onReset,
  status,
}: {
  form: SettingsForm;
  errors: Partial<Record<FormField, string>>;
  onChange: (changes: Partial<SettingsForm>) => void;
  onReset: () => void;
  status: ReactNode;
}) {
  return (
    <div className="panel buy-settings">
      <fieldset className="buy-settings__group">
        <legend>Singles: valued one by one</legend>
        <p className="muted">Cards that match any rule here are singles; everything else is bulk.</p>
        <div className="buy-settings__rarities">
          {(
            [
              ["singleCommon", "Common"],
              ["singleUncommon", "Uncommon"],
              ["singleRare", "Rare"],
              ["singleMythic", "Mythic"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="checkbox">
              <input type="checkbox" checked={form[key]} onChange={(e) => onChange({ [key]: e.target.checked })} />
              <span>{label}</span>
            </label>
          ))}
        </div>
        <div className="buy-settings__row">
          <label className="checkbox">
            <input
              type="checkbox"
              checked={form.useMinPrice}
              onChange={(e) => onChange({ useMinPrice: e.target.checked })}
            />
            <span>Cards worth at least</span>
          </label>
          <NumberField
            label="Single price threshold (€)"
            value={form.minPrice}
            unit="€"
            disabled={!form.useMinPrice}
            error={errors.minPrice}
            onChange={(minPrice) => onChange({ minPrice })}
          />
        </div>
      </fieldset>

      <fieldset className="buy-settings__group">
        <legend>Offer</legend>
        <div className="buy-settings__row">
          <NumberField
            label="Share of a single's value (%)"
            value={form.percent}
            unit="% of their value for singles"
            error={errors.percent}
            onChange={(percent) => onChange({ percent })}
          />
        </div>
        <div className="buy-settings__row">
          <NumberField
            label="Bulk rate (€)"
            value={form.bulkRate}
            unit="€ per"
            error={errors.bulkRate}
            onChange={(bulkRate) => onChange({ bulkRate })}
          />
          <NumberField
            label="Bulk batch (cards)"
            value={form.bulkBatch}
            unit="bulk cards"
            numeric
            error={errors.bulkBatch}
            onChange={(bulkBatch) => onChange({ bulkBatch })}
          />
        </div>
      </fieldset>

      <div className="buy-settings__actions">
        {status}
        <button type="button" className="button button--small" onClick={onReset}>
          Reset to defaults
        </button>
      </div>
    </div>
  );
}

function NumberField({
  label,
  value,
  unit,
  error,
  disabled = false,
  numeric = false,
  onChange,
}: {
  label: string;
  value: string;
  unit: string;
  error?: string;
  disabled?: boolean;
  numeric?: boolean;
  onChange: (value: string) => void;
}) {
  const id = useId();
  const errorId = useId();
  return (
    <span className="number-field">
      <input
        id={id}
        className="number-field__input"
        aria-label={label}
        value={value}
        inputMode={numeric ? "numeric" : "decimal"}
        disabled={disabled}
        aria-invalid={Boolean(error) && !disabled}
        aria-describedby={error && !disabled ? errorId : undefined}
        onChange={(e) => onChange(e.target.value)}
        maxLength={12}
        autoComplete="off"
      />
      <span className="number-field__unit">{unit}</span>
      {error && !disabled && (
        <span id={errorId} className="number-field__error" role="alert">
          {error}
        </span>
      )}
    </span>
  );
}

function OfferView({ offer, updating, exportButton }: { offer: BuyOffer; updating: boolean; exportButton: ReactNode }) {
  const s = offer.summary;
  return (
    <div className={updating ? "offer offer--updating" : "offer"}>
      <h2>Offer</h2>
      <div className="table-wrap offer__summary">
        <table className="table">
          <thead>
            <tr>
              <th scope="col" />
              <th scope="col" className="num">
                Cards
              </th>
              <th scope="col" className="num">
                Value
              </th>
              <th scope="col" className="num">
                Offer
              </th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">Singles</th>
              <td className="num">{formatCount(s.single_cards)}</td>
              <td className="num">{formatEuro(s.single_market_value)}</td>
              <td className="num">{formatEuro(s.single_offer)}</td>
            </tr>
            <tr>
              <th scope="row">Bulk</th>
              <td className="num">{formatCount(s.bulk_cards)}</td>
              <td className="num">{formatEuro(s.bulk_market_value)}</td>
              <td className="num">{formatEuro(s.bulk_offer)}</td>
            </tr>
            <tr className="offer__total">
              <th scope="row">Total</th>
              <td className="num">{formatCount(s.single_cards + s.bulk_cards)}</td>
              <td className="num">{formatEuro(s.single_market_value + s.bulk_market_value)}</td>
              <td className="num">{formatEuro(s.total_offer)}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <p className="muted">Values are the prices in the seller&apos;s list.</p>
      {exportButton}

      <h2>Singles, most valuable first</h2>
      {offer.singles_total > offer.singles.length && (
        <p className="muted">
          Showing the {offer.singles.length} most valuable of {formatCount(offer.singles_total)} singles; the CSV has
          all of them.
        </p>
      )}
      {offer.singles.length === 0 ? (
        <p className="muted">No singles in this list.</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Set</th>
                <th scope="col">Rarity</th>
                <th scope="col" className="num">
                  Qty
                </th>
                <th scope="col" className="num">
                  Unit price
                </th>
                <th scope="col" className="num">
                  Offer
                </th>
              </tr>
            </thead>
            <tbody>
              {offer.singles.map((card, index) => (
                <tr key={`${index}-${card.name}`}>
                  <td>{card.name}</td>
                  <td>{card.set_code}</td>
                  <td>{card.rarity}</td>
                  <td className="num">{formatCount(card.quantity)}</td>
                  <td className="num">{formatEuro(card.unit_price)}</td>
                  <td className="num">{formatEuro(card.offer)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
