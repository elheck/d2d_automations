// Inventory import: choose the Cardmarket export, see what it changes, then
// import it. The server checks the same file again when it is confirmed,
// takes a backup first and refuses when the data changed in between.

import { useQueryClient } from "@tanstack/react-query";
import { useState, type ChangeEvent } from "react";
import { ApiError } from "../api/client";
import { useConfirmImport, useDataInfo, useDiscardImport, useImportHistory, usePreviewImport } from "../api/queries";
import type { InventoryImportResult, InventoryPreview } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { formatCount } from "../format";
import { markSeen } from "../inventoryVersion";
import {
  CHECK_AGAIN_CODES,
  importFileProblem,
  KIND_LABELS,
  paceText,
  typedCount,
  unusualText,
} from "./inventoryImport";
import { formatTime } from "./UsersPage";

interface ChosenFile {
  name: string;
  csv: string;
}

export function InventoryImportPage() {
  const dataInfo = useDataInfo();
  const preview = usePreviewImport();
  const confirm = useConfirmImport();
  const discard = useDiscardImport();
  const queryClient = useQueryClient();
  const [file, setFile] = useState<ChosenFile | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState<InventoryImportResult | null>(null);

  function check(chosen: ChosenFile) {
    setTyped("");
    confirm.reset();
    preview.mutate({ file_name: chosen.name, csv: chosen.csv });
  }

  async function chooseFile(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const chosen = input.files?.[0];
    // Choosing the same file again (a fresh export with the same name) must check it again.
    input.value = "";
    if (!chosen) return;
    const problem = importFileProblem(chosen);
    setDone(null);
    if (problem) {
      setFile(null);
      preview.reset();
      setFileError(problem);
      return;
    }
    try {
      const loaded = { name: chosen.name, csv: await chosen.text() };
      setFileError(null);
      setFile(loaded);
      check(loaded);
    } catch {
      setFileError("The file could not be read.");
    }
  }

  function importNow(confirmSold?: string) {
    if (!file) return;
    confirm.mutate(
      { file_name: file.name, csv: file.csv, ...(confirmSold === undefined ? {} : { confirm_sold: confirmSold }) },
      {
        onSuccess: (result) => {
          markSeen(result.version);
          setDone(result);
          setFile(null);
          preview.reset();
          void queryClient.invalidateQueries();
        },
      },
    );
  }

  function cancel() {
    discard.mutate();
    setFile(null);
    preview.reset();
    confirm.reset();
  }

  const checked = file ? preview.data : undefined;
  const confirmError = confirm.error instanceof ApiError ? confirm.error : null;
  const canCheckAgain = file && confirmError && CHECK_AGAIN_CODES.has(confirmError.code);

  return (
    <section className="page">
      <h1>Inventory import</h1>
      <p className="muted">
        Upload the inventory export from Cardmarket (the <code>inventory-report-…csv</code> file). Nothing is written
        until you have seen what it changes and confirmed it.
      </p>
      {dataInfo.data?.test_copy && (
        <p className="notice notice--warning">
          <strong>Test copy.</strong> This server works on a copy of the desktop app&apos;s database. Imports here only
          change this copy and are thrown away at the move to the server; keep importing in the desktop app until then.
        </p>
      )}

      <div className="panel">
        <p className="panel__title" id="import-file-title">
          Cardmarket inventory export (CSV)
        </p>
        <div className="file-picker">
          <input
            id="import-file"
            className="visually-hidden"
            type="file"
            accept=".csv,text/csv"
            aria-labelledby="import-file-title"
            onChange={chooseFile}
          />
          <label htmlFor="import-file" className="button">
            {file ? "Choose another file…" : "Choose export…"}
          </label>
          <span className="muted">{file ? file.name : "No file chosen"}</span>
        </div>
        {fileError && (
          <p className="error" role="alert">
            {fileError}
          </p>
        )}
        {preview.isPending && <p className="loading">Checking the file…</p>}
        <ErrorMessage error={file ? preview.error : null} />
      </div>

      {done && <ImportDone result={done} />}

      {checked && (
        <Preview
          preview={checked}
          typed={typed}
          onTyped={setTyped}
          busy={confirm.isPending}
          onImport={importNow}
          onCancel={cancel}
        />
      )}
      {confirm.error && (
        <div className="panel">
          <ErrorMessage error={confirm.error} />
          {canCheckAgain && (
            <button type="button" className="button" onClick={() => check(file)}>
              Check the file again
            </button>
          )}
        </div>
      )}

      <History />
    </section>
  );
}

function Preview({
  preview: p,
  typed,
  onTyped,
  busy,
  onImport,
  onCancel,
}: {
  preview: InventoryPreview;
  typed: string;
  onTyped: (value: string) => void;
  busy: boolean;
  onImport: (confirmSold?: string) => void;
  onCancel: () => void;
}) {
  const n = p.numbers;
  const typedRight = typedCount(typed) === n.copies_sold;
  const rows: [string, string, string?][] = [
    ["Listings in the file", formatCount(p.rows)],
    ["Copies in the file", formatCount(p.copies_in_file)],
    ["Copies in stock", `${formatCount(n.copies_before)} → ${formatCount(n.copies_after)}`],
    ["New listings", formatCount(n.new_variants)],
    ["Listings updated", formatCount(n.updated_variants)],
    ["Listings gone (now 0)", formatCount(n.zeroed_variants)],
    ["Copies that will count as sold", formatCount(n.copies_sold), "import-numbers__key"],
  ];
  if (n.bulk_removed > 0) rows.push(["Copies gone into bulk lots (not counted as sold)", formatCount(n.bulk_removed)]);
  rows.push(["Price changes", formatCount(n.price_changes)]);

  return (
    <section className="panel" aria-label="Check of the file">
      <p className="panel__title">
        Check of {p.file_name} · {p.category}
      </p>
      {p.new_category && (
        <p className="notice notice--warning">New product category “{p.category}”: nothing of it is in the data yet.</p>
      )}
      <table className="table import-numbers">
        <tbody>
          {rows.map(([label, value, className]) => (
            <tr key={label} className={className}>
              <th scope="row">{label}</th>
              <td className="num">{value}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted">{paceText(p.pace)}</p>
      {p.unusual && !p.blocked && (
        <p className="notice notice--warning" role="alert">
          {unusualText(p.unusual, n.copies_sold)}
        </p>
      )}

      {p.blocked ? (
        <div className="notice notice--danger" role="alert">
          <p>
            <strong>Blocked:</strong> this import would count {formatCount(n.copies_sold)} of{" "}
            {formatCount(n.copies_before)} copies, more than half the stock, as sold. That usually means a cut-off or
            wrong file. Only if the stock really dropped this much, type {formatCount(n.copies_sold)} below to import it
            anyway.
          </p>
          <form
            className="import-force"
            onSubmit={(event) => {
              event.preventDefault();
              if (typedRight) onImport(typed);
            }}
          >
            <label htmlFor="import-force-count">Copies that will count as sold</label>
            <input
              id="import-force-count"
              className="input"
              inputMode="numeric"
              autoComplete="off"
              value={typed}
              onChange={(event) => onTyped(event.target.value)}
            />
            <button type="submit" className="button button--danger" disabled={!typedRight || busy}>
              Import anyway
            </button>
            <button type="button" className="button button--ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </button>
          </form>
        </div>
      ) : (
        <div className="import-actions">
          <button type="button" className="button button--primary" disabled={busy} onClick={() => onImport()}>
            {busy ? "Importing…" : "Import"}
          </button>
          <button type="button" className="button button--ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
        </div>
      )}
      <p className="muted small">
        A backup of the database is taken right before the import. This check can be confirmed for {p.valid_minutes}{" "}
        minutes (until {new Date(p.expires_at).toLocaleTimeString()}).
      </p>
    </section>
  );
}

function ImportDone({ result }: { result: InventoryImportResult }) {
  const n = result.numbers;
  return (
    <p className="notice notice--success" role="status">
      Imported ({result.category}
      {result.forced ? ", forced" : ""}): {formatCount(n.new_variants)} new listings, {formatCount(n.updated_variants)}{" "}
      updated, {formatCount(n.zeroed_variants)} gone, {formatCount(n.copies_sold)} copies counted as sold. Every page
      now shows the new data.
    </p>
  );
}

function History() {
  const history = useImportHistory();
  if (!history.data || history.data.length === 0) return null;
  return (
    <>
      <h2>Recent changes of the inventory data</h2>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th scope="col">When</th>
              <th scope="col">What</th>
              <th scope="col">Category</th>
              <th scope="col">By</th>
              <th scope="col" className="num">
                Sold
              </th>
              <th scope="col" className="num">
                New
              </th>
              <th scope="col" className="num">
                Gone
              </th>
            </tr>
          </thead>
          <tbody>
            {history.data.map((v) => (
              <tr key={v.id}>
                <td className="nowrap">{formatTime(v.created_at)}</td>
                <td>{KIND_LABELS[v.kind] ?? v.kind}</td>
                <td>{v.category}</td>
                <td>{v.user_name}</td>
                <td className="num">{v.kind.endsWith("import") ? formatCount(v.copies_sold) : "—"}</td>
                <td className="num">{v.kind.endsWith("import") ? formatCount(v.new_variants) : "—"}</td>
                <td className="num">{v.kind.endsWith("import") ? formatCount(v.zeroed_variants) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
