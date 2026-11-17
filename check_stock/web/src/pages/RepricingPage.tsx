// Repricing: one shared, ordered ruleset prices the whole stock of the last
// inventory import. Editors change a draft of their own (saved on the
// server as they type) and publish it; everybody reviews what the
// published rules propose, ticks the changes to export and downloads the
// new prices plus an undo file. Exports are kept on the server.

import { useCallback, useState } from "react";
import { ApiError } from "../api/client";
import {
  can,
  downloadPriceExport,
  downloadRuleset,
  saveRulesetDraftOnExit,
  useDropRulesetDraft,
  useMarketStatus,
  useMe,
  useOpenRulesetFile,
  usePublishRuleset,
  useRepricingExports,
  useRepricingReview,
  useRepricingSetup,
  useSaveRulesetDraft,
} from "../api/queries";
import type { RepricingSetup, Ruleset } from "../api/types";
import { ErrorMessage } from "../components/ErrorMessage";
import { MarketPanel } from "../components/MarketPanel";
import { Modal } from "../components/Modal";
import { formatCount } from "../format";
import { useAutoSave } from "../useAutoSave";
import { useDebounced } from "../useDebounced";
import { sourceText } from "./bins";
import { saveDownload } from "./buyHelper";
import { sameRuleset } from "./repricing";
import { ExportDialog, FIRST_VIEW, ListingDialog, queryOf, ReviewPanel, type ReviewState } from "./RepricingReview";
import { RulesPanel } from "./RepricingRules";

type Tab = "prices" | "exports";

export function RepricingPage() {
  const { data: me } = useMe();
  const setup = useRepricingSetup();
  const market = useMarketStatus("/api/repricing/market");
  const [tab, setTab] = useState<Tab>("prices");
  const canEdit = can(me, "repricing.edit_rules");
  const canExport = can(me, "repricing.export");

  return (
    <section className="page page--wide">
      <h1>Repricing</h1>
      {canExport && (
        <div className="tabs" role="tablist" aria-label="Repricing">
          {(
            [
              ["prices", "Rules & prices"],
              ["exports", "Exports"],
            ] as [Tab, string][]
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={tab === key}
              className={tab === key ? "tab tab--active" : "tab"}
              onClick={() => setTab(key)}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      <ErrorMessage error={setup.error ?? market.error} />
      {tab === "exports" && canExport && <Exports />}
      {tab === "prices" && setup.data && (
        <>
          {setup.data.source ? (
            <p className="data-as-of">{sourceText(setup.data.source)}</p>
          ) : (
            <p className="notice notice--warning">
              Repricing needs an inventory import on the server (Tools → Inventory import).
            </p>
          )}
          {market.data && setup.data.source && <MarketPanel base="/api/repricing/market" status={market.data} />}
          {setup.data.source && (
            <Workbench
              key={`${setup.data.source.version}`}
              setup={setup.data}
              canEdit={canEdit}
              canExport={canExport}
              marketVersion={market.data?.loaded?.version ?? null}
            />
          )}
        </>
      )}
    </section>
  );
}

// ── Rules and review side by side ──────────────────────────────────────────

function Workbench({
  setup,
  canEdit,
  canExport,
  marketVersion,
}: {
  setup: RepricingSetup;
  canEdit: boolean;
  canExport: boolean;
  marketVersion: number | null;
}) {
  const published = setup.published;
  const serverDraft = setup.draft;
  // The editor's working copy (viewers see the published rules).
  const [ruleset, setRuleset] = useState<Ruleset>(serverDraft?.ruleset ?? published.ruleset);
  const [base, setBase] = useState(serverDraft?.base_revision ?? published.revision);
  const [seen, setSeen] = useState(published);
  const [selected, setSelected] = useState<number | null>(null);
  const [viewDraft, setViewDraft] = useState(true);
  const [review, setReview] = useState<ReviewState>(FIRST_VIEW);
  const [search, setSearch] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  // Counts saved drafts, so the review asks again after each one.
  const [saves, setSaves] = useState(0);
  const save = useSaveRulesetDraft();
  const saveNow = (r: Ruleset) =>
    save.mutateAsync({ base_revision: base, ruleset: r }).finally(() => setSaves((n) => n + 1));

  // Another revision was published: mine (it now equals my copy), or
  // someone else's while I had no changes of my own (then follow it).
  if (published.revision !== seen.revision) {
    setSeen(published);
    if (sameRuleset(ruleset, published.ruleset)) {
      setBase(published.revision);
    } else if (!serverDraft && sameRuleset(ruleset, seen.ruleset)) {
      setRuleset(published.ruleset);
      setBase(published.revision);
    }
  }

  useAutoSave(
    canEdit ? ruleset : null,
    serverDraft?.ruleset ?? published.ruleset,
    (r) => void saveNow(r).catch(() => undefined),
    (r) => saveRulesetDraftOnExit({ base_revision: base, ruleset: r }),
  );

  const settledSearch = useDebounced(search, 300);
  const state = settledSearch === review.search ? review : { ...review, search: settledSearch, offset: 0 };
  const showDraft = canEdit && serverDraft !== null && viewDraft;
  // Editors with a draft may look at the published rules: read-only then.
  const showingPublished = canEdit && serverDraft !== null && !viewDraft;
  const editable = canEdit && !showingPublished;
  const draftKey = `${published.revision}:${saves}:${marketVersion ?? "-"}`;
  const page = useRepricingReview(queryOf(state, showDraft), draftKey);
  const closeListing = useCallback(() => setOpen(null), []);
  const closeExport = useCallback(() => setExporting(false), []);

  return (
    <>
      <RulesetBar
        setup={setup}
        canEdit={canEdit}
        ruleset={ruleset}
        base={base}
        saving={save.isPending}
        saveError={save.error}
        viewDraft={viewDraft}
        onViewDraft={setViewDraft}
        onReplace={(r, b) => {
          setRuleset(r);
          setBase(b);
          setSelected(null);
        }}
        flush={() => saveNow(ruleset)}
      />
      <div className="repricing-layout">
        <RulesPanel
          ruleset={editable ? ruleset : published.ruleset}
          onChange={editable ? setRuleset : null}
          options={setup.options}
          views={page.data?.rules}
          selected={selected}
          onSelect={setSelected}
          onShowRule={(i) => setReview({ ...FIRST_VIEW, filter: "all", rule: i })}
        />
        <div className="repricing-main">
          <ErrorMessage error={page.error} />
          {page.data && (
            <ReviewPanel
              page={page.data}
              fetching={page.isFetching}
              state={state}
              search={search}
              onSearch={setSearch}
              onState={(s) => {
                setReview(s);
                setSearch(s.search);
              }}
              rules={page.data.rules}
              canExport={canExport}
              onOpen={setOpen}
              onExport={() => setExporting(true)}
            />
          )}
        </div>
      </div>
      {open && page.data && (
        <ListingDialog
          pile={open}
          draft={showDraft}
          draftKey={draftKey}
          version={page.data.source.version}
          canExport={canExport}
          onClose={closeListing}
        />
      )}
      {exporting && <ExportDialog draftKey={draftKey} onClose={closeExport} />}
    </>
  );
}

// ── Draft, publish, files ──────────────────────────────────────────────────

function RulesetBar({
  setup,
  canEdit,
  ruleset,
  base,
  saving,
  saveError,
  viewDraft,
  onViewDraft,
  onReplace,
  flush,
}: {
  setup: RepricingSetup;
  canEdit: boolean;
  ruleset: Ruleset;
  base: number;
  saving: boolean;
  saveError: unknown;
  viewDraft: boolean;
  onViewDraft: (draft: boolean) => void;
  onReplace: (ruleset: Ruleset, base: number) => void;
  /** Saves the working copy now (before publishing). */
  flush: () => Promise<{ draft: RepricingSetup["draft"] }>;
}) {
  const published = setup.published;
  const draft = setup.draft;
  const publish = usePublishRuleset();
  const drop = useDropRulesetDraft();
  const openFile = useOpenRulesetFile();
  const [conflict, setConflict] = useState<string | null>(null);
  const [undo, setUndo] = useState<{ ruleset: Ruleset; base: number } | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [downloadError, setDownloadError] = useState<unknown>(null);
  const changed = canEdit && !sameRuleset(ruleset, published.ruleset);

  const doPublish = async (expected: number) => {
    setConflict(null);
    try {
      const saved = await flush();
      if (!saved.draft) return;
      await publish.mutateAsync(expected);
      setUndo(null);
    } catch (e) {
      if (e instanceof ApiError && e.code === "ruleset_conflict") setConflict(e.message);
    }
  };

  const onFile = async (file: File | undefined) => {
    setFileError(null);
    if (!file) return;
    if (file.size > setup.options.max_ruleset_bytes) {
      setFileError("The file is too large to be a repricing ruleset (at most 1 MB).");
      return;
    }
    const before = { ruleset, base };
    openFile.mutate(await file.text(), {
      onSuccess: (saved) => {
        setUndo(before);
        onReplace(saved.draft?.ruleset ?? published.ruleset, saved.draft?.base_revision ?? published.revision);
        onViewDraft(true);
      },
    });
  };

  const download = async (fromDraft: boolean) => {
    setDownloadError(null);
    try {
      saveDownload(await downloadRuleset(fromDraft));
    } catch (e) {
      setDownloadError(e);
    }
  };

  const by = published.updated_by
    ? `revision ${published.revision} by ${published.updated_by}, ${new Date(published.updated_at ?? "").toLocaleString()}`
    : "the starter rules (never published)";
  return (
    <div className={changed ? "panel ruleset-bar ruleset-bar--draft" : "panel ruleset-bar"}>
      <div className="ruleset-bar__state">
        {changed ? (
          <>
            <strong>Unpublished changes</strong>
            <span className="muted small">
              {saving ? "Saving…" : "Saved as your draft"} · based on revision {base}
            </span>
          </>
        ) : (
          <>
            <strong>Published rules</strong>
            <span className="muted small">{by}</span>
          </>
        )}
      </div>
      {changed && published.revision > base && (
        <p className="notice notice--warning ruleset-bar__wide">
          {published.updated_by ?? "Someone"} published revision {published.revision} after you started; publishing
          yours replaces it.
        </p>
      )}
      {canEdit && draft && (
        <div className="segmented" role="group" aria-label="Show prices of">
          {(
            [
              [true, "My draft"],
              [false, "Published"],
            ] as [boolean, string][]
          ).map(([value, label]) => (
            <button
              key={label}
              type="button"
              className={viewDraft === value ? "segmented__button segmented__button--on" : "segmented__button"}
              aria-pressed={viewDraft === value}
              onClick={() => onViewDraft(value)}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      <div className="ruleset-bar__actions">
        {changed && (
          <>
            <button
              type="button"
              className="button button--primary"
              disabled={publish.isPending || saving}
              onClick={() => void doPublish(published.revision)}
            >
              Publish for everyone
            </button>
            <button
              type="button"
              className="button button--ghost"
              disabled={drop.isPending}
              onClick={() =>
                drop.mutate(undefined, {
                  onSuccess: () => onReplace(published.ruleset, published.revision),
                })
              }
            >
              Discard my changes
            </button>
          </>
        )}
        {undo && (
          <button
            type="button"
            className="button button--ghost"
            onClick={() => {
              onReplace(undo.ruleset, undo.base);
              setUndo(null);
            }}
          >
            Undo opening the file
          </button>
        )}
        <details className="ruleset-bar__file">
          <summary className="button button--small">File…</summary>
          <div className="ruleset-bar__menu">
            <button type="button" className="button button--small" onClick={() => void download(false)}>
              Download the published rules (JSON)
            </button>
            {canEdit && draft && (
              <button type="button" className="button button--small" onClick={() => void download(true)}>
                Download my draft (JSON)
              </button>
            )}
            {canEdit && (
              <label className="button button--small file-picker">
                Open a rules file…
                <input
                  type="file"
                  accept=".json,application/json"
                  className="visually-hidden"
                  onChange={(e) => {
                    void onFile(e.target.files?.[0]);
                    e.target.value = "";
                  }}
                />
              </label>
            )}
            {canEdit && <span className="muted small">An opened file becomes your draft; nothing is published.</span>}
          </div>
        </details>
      </div>
      {fileError && (
        <p className="error ruleset-bar__wide" role="alert">
          {fileError}
        </p>
      )}
      <div className="ruleset-bar__wide">
        <ErrorMessage error={saveError ?? publish.error ?? drop.error ?? openFile.error ?? downloadError} />
      </div>
      {conflict && (
        <Modal title="Someone published meanwhile" onClose={() => setConflict(null)}>
          <p>{conflict}</p>
          <p className="muted">
            Publishing yours replaces their rules for everyone. Discarding yours loads theirs into the editor.
          </p>
          <div className="import-actions">
            <button type="button" className="button button--danger" onClick={() => void doPublish(published.revision)}>
              Publish mine anyway
            </button>
            <button
              type="button"
              className="button"
              onClick={() =>
                drop.mutate(undefined, {
                  onSuccess: () => {
                    setConflict(null);
                    onReplace(published.ruleset, published.revision);
                  },
                })
              }
            >
              Discard mine, use theirs
            </button>
            <button type="button" className="button button--ghost" onClick={() => setConflict(null)}>
              Cancel
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

// ── Kept exports ───────────────────────────────────────────────────────────

function Exports() {
  const list = useRepricingExports(true);
  const [error, setError] = useState<unknown>(null);
  const get = async (id: number, undo: boolean, fileName: string) => {
    setError(null);
    try {
      saveDownload(await downloadPriceExport(id, undo, fileName));
    } catch (e) {
      setError(e);
    }
  };
  return (
    <>
      <p className="muted">The last 20 exports, with their undo files (current prices of the time).</p>
      <ErrorMessage error={list.error ?? error} />
      {list.data && list.data.length === 0 && <p className="muted">No exports yet.</p>}
      {list.data && list.data.length > 0 && (
        <div className="table-wrap">
          <table className="table" aria-label="Exports">
            <thead>
              <tr>
                <th scope="col">When</th>
                <th scope="col">By</th>
                <th scope="col" className="num">
                  Changes
                </th>
                <th scope="col">Rules</th>
                <th scope="col">Files</th>
              </tr>
            </thead>
            <tbody>
              {list.data.map((e) => (
                <tr key={e.id}>
                  <td className="nowrap">{new Date(e.created_at).toLocaleString()}</td>
                  <td>{e.username}</td>
                  <td className="num">
                    {formatCount(e.changes)}{" "}
                    <span className="muted small">
                      (↑{e.raises} ↓{e.lowers})
                    </span>
                  </td>
                  <td>revision {e.revision}</td>
                  <td>
                    <div className="import-actions">
                      <button
                        type="button"
                        className="button button--small"
                        onClick={() => void get(e.id, false, e.file_name)}
                      >
                        New prices
                      </button>
                      <button
                        type="button"
                        className="button button--small"
                        onClick={() => void get(e.id, true, e.undo_file_name)}
                      >
                        Undo file
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
