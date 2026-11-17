// One precon: its value figures, the EV history (weekly) and every card with
// its price. Uses the settings saved on the ranking page.

import { Link, useParams } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { usePreconHistory, usePreconProduct, usePreconSettings, usePreconStatus } from "../api/queries";
import type { CardLine, DeckDetail, ProductDetail } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { LineChart } from "../components/LineChart";
import { formatEuro } from "../format";
import {
  BOARD_LABELS,
  columnLabel,
  coverageText,
  formatRatio,
  isCardmarketUrl,
  ratioTone,
  soldText,
  soldTitle,
} from "./preconEv";

/** The id in the address, if it is one. */
export function productId(raw: string | undefined): number | null {
  return raw && /^\d{1,12}$/.test(raw) ? Number(raw) : null;
}

export function PreconProductPage() {
  const params = useParams({ strict: false }) as { id?: string };
  const id = productId(params.id);
  const settings = usePreconSettings();
  const status = usePreconStatus();
  const version = status.data?.loaded?.version ?? null;
  const current = settings.data?.settings ?? null;
  const product = usePreconProduct(id ?? 0, id === null ? null : current, version);
  const hasDecks = (product.data?.decks.length ?? 0) > 0;
  const history = usePreconHistory(id ?? 0, hasDecks ? current : null, version);

  return (
    <section className="page page--wide">
      <p>
        <Link to="/precon-ev">⏴ Back to the ranking</Link>
      </p>
      {id === null && <p className="error">This is not a precon address.</p>}
      <ErrorMessage error={settings.error ?? status.error} />
      {status.data && !status.data.loaded && (
        <p className="muted">
          The precons are not loaded yet. <Link to="/precon-ev">Load them on the ranking page.</Link>
        </p>
      )}
      <ErrorMessage error={product.error} />
      {product.isPending && version !== null && id !== null && <p className="loading">Loading…</p>}
      {product.data && current && (
        <>
          <Header detail={product.data} />
          <p className="muted">
            Card price {columnLabel(current.field)} · sealed {columnLabel(current.sealed_field)} · bulk cutoff{" "}
            {formatEuro(current.cutoff)} · foil premium {current.foil_needs_sales ? "only if sold" : "always"} (change
            on the ranking page)
          </p>
          {product.data.row.state !== "ready" ? (
            <p className="warning">{product.data.row.note}</p>
          ) : (
            <>
              <Kpis detail={product.data} cutoff={current.cutoff} />
              <h2>EV history (weekly)</h2>
              {history.isPending && <p className="loading">Loading the price history…</p>}
              <ErrorMessage error={history.error} />
              {history.data &&
                (history.data.points.length === 0 ? (
                  <p className="muted">No price history yet.</p>
                ) : (
                  <>
                    <LineChart
                      label="EV, sealed price and Top 5 per week"
                      dates={history.data.points.map((p) => p.date)}
                      format={(v) => formatEuro(v)}
                      tickFormat={(v) => `${Math.round(v)} €`}
                      series={[
                        {
                          id: "ev",
                          label: "EV",
                          className: "chart-series--ev",
                          values: history.data.points.map((p) => p.ev),
                        },
                        {
                          id: "sealed",
                          label: "Sealed",
                          className: "chart-series--sealed",
                          values: history.data.points.map((p) => p.sealed),
                        },
                        {
                          id: "top5",
                          label: "Top 5",
                          className: "chart-series--top5",
                          values: history.data.points.map((p) => p.top5),
                        },
                      ]}
                    />
                    <p className="muted small">
                      Weeks where under 90% of the copies had a price (e.g. before the release) are left blank.
                    </p>
                  </>
                ))}
              {product.data.decks.length === 1 ? (
                <>
                  <h2>Cards</h2>
                  <CardTable deck={product.data.decks[0]} />
                </>
              ) : (
                product.data.decks.map((deck) => (
                  <section key={deck.name}>
                    <h2>
                      {deck.name}: EV {formatEuro(deck.ev)}
                    </h2>
                    <CardTable deck={deck} />
                  </section>
                ))
              )}
            </>
          )}
        </>
      )}
    </section>
  );
}

function Header({ detail }: { detail: ProductDetail }) {
  const row = detail.row;
  return (
    <>
      <h1>{row.name}</h1>
      <p className="muted">
        {row.set_label || "—"} · released {row.release_date ?? "—"} · Cardmarket #{row.id_product}
        {detail.decks.length > 0 && ` · ${detail.decks.map((d) => d.name).join(", ")}`}
      </p>
    </>
  );
}

function Kpi({ label, children, tone }: { label: string; children: ReactNode; tone?: string }) {
  return (
    <div className={tone ? `kpi kpi--${tone}` : "kpi"}>
      <span className="kpi__label">{label}</span>
      <span className="kpi__value">{children}</span>
    </div>
  );
}

function Kpis({ detail, cutoff }: { detail: ProductDetail; cutoff: number }) {
  const row = detail.row;
  const ev = row.ev;
  if (!ev) return null;
  return (
    <div className="kpis">
      <Kpi label="Sealed" tone="sealed">
        {isCardmarketUrl(row.cardmarket_url) ? (
          <a href={row.cardmarket_url} target="_blank" rel="noopener noreferrer">
            {formatEuro(row.sealed_price)}
          </a>
        ) : (
          formatEuro(row.sealed_price)
        )}
      </Kpi>
      <Kpi label={`EV (≥ ${formatEuro(cutoff)})`} tone="ev">
        {formatEuro(ev.ev)}
      </Kpi>
      <Kpi label="Cheapest offer" tone={row.stale_sealed !== null ? "warning" : undefined}>
        {formatEuro(row.sealed_low)}
      </Kpi>
      <Kpi label="EV/Sealed" tone={ratioTone(row) === "good" ? "good" : undefined}>
        {formatRatio(row.ratio)}
        {ratioTone(row) === "stale" && " ⚠"}
      </Kpi>
      <Kpi label="Liquid EV">{formatEuro(ev.liquid)}</Kpi>
      <Kpi label="Top 5" tone="top5">
        {formatEuro(ev.top5)}
      </Kpi>
      <Kpi label="Gross (no cutoff)">{formatEuro(ev.gross)}</Kpi>
      <Kpi label="Priced copies">
        {ev.priced_copies}/{ev.total_copies} ({coverageText(ev)})
      </Kpi>
      <Kpi label="Counted in EV">{ev.counted_copies} copies</Kpi>
    </div>
  );
}

function CardName({ card }: { card: CardLine }) {
  const name = (
    <>
      {card.name}
      {card.is_foil && (
        <span className="foil" title="Foil">
          {" "}
          ✦
        </span>
      )}
    </>
  );
  return isCardmarketUrl(card.cardmarket_url) ? (
    <a href={card.cardmarket_url} target="_blank" rel="noopener noreferrer">
      {name}
    </a>
  ) : (
    name
  );
}

function CardTable({ deck }: { deck: DeckDetail | undefined }) {
  if (!deck) return null;
  return (
    <div className="table-wrap">
      <table className="table precon-cards">
        <thead>
          <tr>
            <th scope="col" className="num">
              #
            </th>
            <th scope="col">Card</th>
            <th scope="col">Set #</th>
            <th scope="col">Board</th>
            <th scope="col" className="num">
              Qty
            </th>
            <th scope="col" className="num">
              Unit
            </th>
            <th scope="col" className="num">
              Line
            </th>
            <th scope="col">Sold</th>
            <th scope="col">Note</th>
          </tr>
        </thead>
        <tbody>
          {deck.cards.map((card) => (
            <tr key={`${card.rank}-${card.name}`} className={card.counted ? undefined : "row--muted"}>
              <td className="num nowrap">
                {card.rank}
                {card.top5 && (
                  <span className="top5" title="One of the five most expensive copies">
                    {" "}
                    ★
                  </span>
                )}
              </td>
              <td>
                <CardName card={card} />
              </td>
              <td className="nowrap">
                {card.set_code.toUpperCase()} #{card.number}
              </td>
              <td>{BOARD_LABELS[card.board]}</td>
              <td className="num">×{card.count}</td>
              <td className="num">{formatEuro(card.unit)}</td>
              <td className="num">{formatEuro(card.line)}</td>
              <td className="nowrap" title={soldTitle(card)}>
                {soldText(card)}
              </td>
              <td className="details">{card.note}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
