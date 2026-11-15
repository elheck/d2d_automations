// Stock Checker: match a wantslist or deck list against the stock of the
// last import, choose the piles, and get a picking list, an invoice list,
// a summary, the stock-update CSV for Cardmarket or an interactive picking
// list. Lists are pasted or read from a file in the browser; deck links are
// not fetched (copy the deck as text instead).

import { useNavigate } from "@tanstack/react-router";
import { useState, type ChangeEvent } from "react";
import { useCreatePicking, useStockCheck, useStockOutput, useStockUpdateCsv } from "../api/queries";
import type { CheckGroup, OutputFormat } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount, formatEuro } from "../format";
import { saveDownload } from "./buyHelper";
import { count, sourceText } from "./bins";
import { allChosen, chosenGroups, LANGUAGES, pileKey, takeHandoff, wantslistFileProblem } from "./stockChecker";

/** "1 copy in 1 pile chosen", "5 copies in 3 piles chosen". */
export function chosenText(copies: number, piles: number): string {
  return `${formatCount(copies)} ${copies === 1 ? "copy" : "copies"} in ${count(piles, "pile")} chosen`;
}

const OUTPUTS: { format: OutputFormat; label: string; file: string }[] = [
  { format: "picking", label: "Picking list", file: "picking_list.txt" },
  { format: "invoice", label: "Invoice list", file: "invoice_list.txt" },
  { format: "summary", label: "Summary", file: "stock_check.txt" },
];

export function StockCheckerPage() {
  const [handed] = useState(() => takeHandoff());
  const [text, setText] = useState("");
  const [fileError, setFileError] = useState<string | null>(null);
  const [language, setLanguage] = useState<string>("English");
  const [only, setOnly] = useState(false);
  const check = useStockCheck();
  const groups: CheckGroup[] | null = check.data?.groups ?? handed;

  async function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const problem = wantslistFileProblem(file);
    if (problem) {
      setFileError(problem);
      return;
    }
    try {
      setText(await file.text());
      setFileError(null);
    } catch {
      setFileError("The file could not be read.");
    }
  }

  return (
    <section className="page page--wide">
      <h1>Stock Checker</h1>
      <div className="panel">
        <label className="field">
          <span>Wantslist or deck list (Cardmarket wantslist, MTGGoldfish, Arena/MTGO, “4 Lightning Bolt”)</span>
          <textarea
            className="wantslist"
            rows={8}
            value={text}
            onChange={(e) => setText(e.target.value)}
            spellCheck={false}
          />
        </label>
        <p className="muted small">
          Deck links are not fetched: open the deck on Archidekt or Moxfield, use Export → copy as text, and paste it
          here.
        </p>
        <div className="precon-controls">
          <div className="file-picker">
            <input
              id="wantslist-file"
              className="visually-hidden"
              type="file"
              accept=".txt,.csv,.dek,text/plain,text/csv"
              aria-label="Read a list from a file"
              onChange={chooseFile}
            />
            <label htmlFor="wantslist-file" className="button">
              Read from a file…
            </label>
          </div>
          <span className="inline-field">
            <label htmlFor="checker-language">Preferred language</label>
            <select id="checker-language" value={language} onChange={(e) => setLanguage(e.target.value)}>
              {LANGUAGES.map((l) => (
                <option key={l}>{l}</option>
              ))}
            </select>
          </span>
          <label className="checkbox">
            <input type="checkbox" checked={only} onChange={(e) => setOnly(e.target.checked)} />
            Only this language
          </label>
          <button
            type="button"
            className="button button--primary"
            disabled={!text.trim() || check.isPending}
            onClick={() => check.mutate({ wantslist: text, language, language_only: only })}
          >
            {check.isPending ? "Checking…" : "Check stock"}
          </button>
        </div>
        {fileError && (
          <p className="error" role="alert">
            {fileError}
          </p>
        )}
        <ErrorMessage error={check.error} />
      </div>

      {handed && !check.data && <p className="notice">The cards you selected in Search.</p>}
      {check.data && (
        <>
          <p className="data-as-of">{sourceText(check.data.source)}</p>
          <p>
            Found <strong>{formatCount(check.data.found)}</strong> of {formatCount(check.data.wanted)} copies.
          </p>
          {check.data.unparseable_total > 0 && (
            <p className="notice notice--warning">
              {check.data.unparseable_total} {check.data.unparseable_total === 1 ? "line is" : "lines are"} no card row:{" "}
              {check.data.unparseable.join(" · ")}
            </p>
          )}
        </>
      )}
      {groups && <Results key={check.data ? "check" : "handoff"} groups={groups} />}
    </section>
  );
}

function Results({ groups }: { groups: CheckGroup[] }) {
  const [chosen, setChosen] = useState(() => allChosen(groups));
  const [discount, setDiscount] = useState("0");
  const [output, setOutput] = useState<{ label: string; file: string; text: string } | null>(null);
  const navigate = useNavigate();
  const makeOutput = useStockOutput();
  const updateCsv = useStockUpdateCsv();
  const picking = useCreatePicking();
  const selected = chosenGroups(groups, chosen);
  const items = selected.flatMap((g) => g.items);
  const discountValue = Number(discount.replace(",", "."));
  const discountOk = Number.isFinite(discountValue) && discountValue >= 0 && discountValue <= 100;
  const missing = groups.filter((g) => g.found < g.wanted);

  function toggle(key: string) {
    setChosen((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  return (
    <>
      {missing.length > 0 && (
        <p className="muted">Missing: {missing.map((g) => `${g.name} ×${g.wanted - g.found}`).join(" · ")}</p>
      )}
      <div className="table-wrap">
        <table className="table" aria-label="Matches">
          <thead>
            <tr>
              <th scope="col">
                <span className="visually-hidden">Chosen</span>
              </th>
              <th scope="col">Card</th>
              <th scope="col">Set</th>
              <th scope="col">Cond.</th>
              <th scope="col">Language</th>
              <th scope="col" className="num">
                Take
              </th>
              <th scope="col" className="num">
                Price
              </th>
              <th scope="col">Location</th>
            </tr>
          </thead>
          <tbody>
            {groups.map((g, gi) => (
              <GroupRows key={`${gi}-${g.name}`} group={g} index={gi} chosen={chosen} onToggle={toggle} />
            ))}
          </tbody>
        </table>
      </div>

      <div className="panel move-controls">
        <p className="panel__title">
          {chosenText(
            items.reduce((n, i) => n + i.quantity, 0),
            items.length,
          )}
        </p>
        <div className="move-controls__buttons">
          {OUTPUTS.map((o) => (
            <button
              key={o.format}
              type="button"
              className="button"
              disabled={items.length === 0 || makeOutput.isPending || (o.format === "summary" && !discountOk)}
              onClick={() =>
                makeOutput.mutate(
                  { format: o.format, discount: discountOk ? discountValue : 0, groups: selected },
                  { onSuccess: (r) => setOutput({ label: o.label, file: o.file, text: r.text }) },
                )
              }
            >
              {o.label}
            </button>
          ))}
          <span className="inline-field">
            <label htmlFor="checker-discount">Discount %</label>
            <input
              id="checker-discount"
              className="number-field__input"
              inputMode="decimal"
              value={discount}
              aria-invalid={!discountOk}
              onChange={(e) => setDiscount(e.target.value)}
            />
          </span>
          <button
            type="button"
            className="button"
            disabled={items.length === 0 || updateCsv.isPending}
            title="Negative quantities: takes the cards off Cardmarket (import into TCG Powertools)"
            onClick={() => updateCsv.mutate({ items }, { onSuccess: saveDownload })}
          >
            Stock-update CSV
          </button>
          <button
            type="button"
            className="button button--primary"
            disabled={items.length === 0 || picking.isPending}
            onClick={() => picking.mutate({ items }, { onSuccess: () => void navigate({ to: "/picking" }) })}
          >
            Interactive picking
          </button>
        </div>
      </div>
      <ErrorMessage error={makeOutput.error ?? updateCsv.error ?? picking.error} />
      {output && (
        <section className="panel" aria-label={output.label}>
          <p className="panel__title">{output.label}</p>
          <pre className="output-text">{output.text}</pre>
          <div className="import-actions">
            <button type="button" className="button" onClick={() => void navigator.clipboard?.writeText(output.text)}>
              Copy
            </button>
            <button
              type="button"
              className="button"
              onClick={() =>
                saveDownload({ blob: new Blob([output.text], { type: "text/plain" }), fileName: output.file })
              }
            >
              Download
            </button>
          </div>
        </section>
      )}
    </>
  );
}

function GroupRows({
  group: g,
  index,
  chosen,
  onToggle,
}: {
  group: CheckGroup;
  index: number;
  chosen: Set<string>;
  onToggle: (key: string) => void;
}) {
  return (
    <>
      <tr className="match-group">
        <th scope="rowgroup" colSpan={8}>
          {g.name}{" "}
          <span className={g.found < g.wanted ? "change--down small" : "muted small"}>
            {g.found} of {g.wanted}
          </span>
        </th>
      </tr>
      {g.piles.map((p) => {
        const key = pileKey(index, p.pile.id);
        return (
          <tr key={key}>
            <td>
              <input
                type="checkbox"
                checked={chosen.has(key)}
                onChange={() => onToggle(key)}
                aria-label={`Choose ${p.pile.name} from ${p.pile.location || "no location"}`}
              />
            </td>
            <td>
              {p.pile.name}
              {p.pile.foil && " ✦"}
            </td>
            <td className="nowrap">{p.pile.set_code}</td>
            <td>{p.pile.condition}</td>
            <td>{p.pile.language}</td>
            <td className="num">×{p.take}</td>
            <td className="num">{formatEuro(p.pile.price)}</td>
            <td className="nowrap">{p.pile.location || "—"}</td>
          </tr>
        );
      })}
    </>
  );
}
