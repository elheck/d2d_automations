// Singles listing: type a set code and collector number (`hou120`, or only
// `120` with a default set), press Enter, see the card with its image and
// prices. The field is cleared and focused again for the next card.

import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useSingleCard, type SingleLookup } from "../api/queries";
import type { SingleCard } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import {
  formatEuro,
  isCardmarketUrl,
  loadDefaultSet,
  PRICE_ROWS,
  priceStatusText,
  resolveCardInput,
  saveDefaultSet,
} from "./singles";

export function SinglesPage() {
  const [defaultSet, setDefaultSet] = useState(loadDefaultSet);
  const [input, setInput] = useState("");
  const [lookup, setLookup] = useState<SingleLookup | null>(null);
  const [invalid, setInvalid] = useState(false);
  const cardField = useRef<HTMLInputElement>(null);
  const card = useSingleCard(lookup);
  const hintId = useId();

  useEffect(() => saveDefaultSet(defaultSet), [defaultSet]);

  const preview = resolveCardInput(input, defaultSet);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!preview) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setLookup({ input: input.trim(), defaultSet: defaultSet.trim(), ...preview });
    // The next card is most likely from the same set.
    setDefaultSet(preview.setCode);
    setInput("");
    cardField.current?.focus();
  }

  return (
    <section className="page">
      <h1>Singles listing</h1>
      <form className="singles-form" onSubmit={submit} noValidate>
        <div className="field singles-form__set">
          <label htmlFor="singles-default-set">Default set</label>
          <input
            id="singles-default-set"
            value={defaultSet}
            onChange={(e) => setDefaultSet(e.target.value.trim().toLowerCase())}
            placeholder="e.g. hou"
            maxLength={10}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
          />
        </div>
        <div className="field singles-form__card">
          <label htmlFor="singles-card">Card</label>
          <input
            id="singles-card"
            ref={cardField}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              setInvalid(false);
            }}
            placeholder={defaultSet ? "e.g. 120" : "e.g. hou120"}
            maxLength={24}
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            // Listing is typing card after card: start in the field.
            autoFocus
            aria-describedby={hintId}
            aria-invalid={invalid}
          />
        </div>
        <button type="submit" className="button button--primary singles-form__submit">
          Look up
        </button>
        <p id={hintId} className="singles-form__hint muted" aria-live="polite">
          {preview
            ? `→ ${preview.setCode.toUpperCase()} #${preview.collectorNumber}`
            : invalid
              ? "Enter a set code and a three-digit number, e.g. hou120, or only the number with a default set."
              : "Set code + three-digit collector number, then Enter."}
        </p>
      </form>

      <div aria-live="polite">
        {lookup && card.isFetching && !card.data && <p className="loading">Looking up the card…</p>}
        {lookup && card.error && (
          <>
            <p className="muted">
              {lookup.setCode.toUpperCase()} #{lookup.collectorNumber}
            </p>
            <ErrorMessage error={card.error} />
          </>
        )}
        {card.data && !card.error && <CardDetails card={card.data} />}
      </div>
    </section>
  );
}

function CardDetails({ card }: { card: SingleCard }) {
  return (
    <article className="single" aria-label={card.name}>
      <CardImage key={card.image_path ?? `${card.set_code}/${card.collector_number}`} card={card} />
      <div className="single__info">
        <h2 className="single__name">{card.name}</h2>
        <p className="single__set">
          {card.set_name} ({card.set_code.toUpperCase()}) #{card.collector_number} · {capitalize(card.rarity)}
        </p>
        {card.mana_cost && <p>Mana cost: {card.mana_cost}</p>}
        {card.type_line && <p>{card.type_line}</p>}

        <h3>Scryfall</h3>
        <p>
          EUR {formatEuro(card.scryfall_eur)} · EUR foil {formatEuro(card.scryfall_eur_foil)}
        </p>

        <h3>Cardmarket</h3>
        {card.cardmarket_id !== null && (
          <p>
            Product {card.cardmarket_id}
            {isCardmarketUrl(card.cardmarket_url) && (
              <>
                {" · "}
                <a href={card.cardmarket_url} target="_blank" rel="noopener noreferrer">
                  Open on Cardmarket
                </a>
              </>
            )}
          </p>
        )}
        <CardmarketPrices card={card} />
      </div>
    </article>
  );
}

function CardImage({ card }: { card: SingleCard }) {
  const [failed, setFailed] = useState(false);
  if (!card.image_path || failed) {
    return <div className="single__image single__image--missing">No image available</div>;
  }
  return (
    <img
      className="single__image"
      src={card.image_path}
      alt={`${card.name} (${card.set_code.toUpperCase()} #${card.collector_number})`}
      onError={() => setFailed(true)}
    />
  );
}

function CardmarketPrices({ card }: { card: SingleCard }) {
  const prices = card.prices;
  if (prices.status !== "found") return <p className="muted">{priceStatusText(prices)}</p>;
  return (
    <>
      <div className="table-wrap single__prices">
        <table className="table">
          <thead>
            <tr>
              <th scope="col">Price</th>
              <th scope="col" className="num">
                Regular
              </th>
              <th scope="col" className="num">
                Foil
              </th>
            </tr>
          </thead>
          <tbody>
            {PRICE_ROWS.map(({ key, label }) => (
              <tr key={key}>
                <th scope="row">{label}</th>
                <td className="num">{formatEuro(prices.regular[key])}</td>
                <td className="num">{formatEuro(prices.foil[key])}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="muted">Prices of {prices.price_date} from the price tracker.</p>
    </>
  );
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
