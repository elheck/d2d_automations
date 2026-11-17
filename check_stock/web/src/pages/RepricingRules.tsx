// The rules column of the Repricing page: the ordered rules (first enabled
// match wins), each rule's conditions and price, the shop-wide limits and
// the condition/language factors. Read-only without `onChange`.

import { useId, type ReactNode } from "react";
import type {
  Guardrails,
  PriceField,
  RepricingOptions,
  Rule,
  RuleCondition,
  RulePrice,
  Ruleset,
  RuleView,
} from "../api/types";
import { Chip, Field, NumField, RuleGroup } from "../components/Controls";
import { formatCount } from "../format";
import {
  addRule,
  BASE_KINDS,
  baseKind,
  baseOf,
  blankRule,
  conditionKind,
  deleteRule,
  duplicateRule,
  freeTemplates,
  moveRule,
  signedEuro,
  templateOf,
  updateRule,
  type BaseKind,
} from "./repricing";

const PRICE = { min: 0, max: 100_000, optional: true };
const PERCENT = { min: 0, max: 1000 };
const DAYS = { min: 0, max: 36_500, integer: true };

interface RulesProps {
  ruleset: Ruleset;
  /** `null`: read-only. */
  onChange: ((ruleset: Ruleset) => void) | null;
  options: RepricingOptions;
  /** What each rule did (from the review; may lag behind an edit). */
  views: RuleView[] | undefined;
  selected: number | null;
  onSelect: (index: number | null) => void;
  /** Shows only the listings this rule priced in the table. */
  onShowRule: (index: number) => void;
}

export function RulesPanel({ ruleset, onChange, options, views, selected, onSelect, onShowRule }: RulesProps) {
  const edit = onChange;
  return (
    <section className="panel repricing-rules" aria-label="Rules">
      <div className="rule-group">
        <h3 className="rule-group__title">Rules · first match wins</h3>
        {ruleset.rules.length === 0 && <p className="muted small">No rules: every price stays.</p>}
        <ol className="rule-list">
          {ruleset.rules.map((rule, i) => (
            <RuleCard
              key={i}
              index={i}
              rule={rule}
              view={views?.[i]}
              open={selected === i}
              count={ruleset.rules.length}
              options={options}
              onToggle={() => onSelect(selected === i ? null : i)}
              onShow={() => onShowRule(i)}
              onChange={edit ? (r) => edit(updateRule(ruleset, i, r)) : null}
              onMove={
                edit
                  ? (step) => {
                      edit(moveRule(ruleset, i, step));
                      onSelect(i + step);
                    }
                  : null
              }
              onDuplicate={
                edit
                  ? () => {
                      edit(duplicateRule(ruleset, i));
                      onSelect(i + 1);
                    }
                  : null
              }
              onDelete={
                edit
                  ? () => {
                      edit(deleteRule(ruleset, i));
                      onSelect(null);
                    }
                  : null
              }
            />
          ))}
        </ol>
        {edit && (
          <select
            className="rule-select"
            aria-label="Add a rule"
            value=""
            onChange={(e) => {
              const pick = e.target.value;
              if (pick === "") return;
              const rule = pick === "blank" ? blankRule() : options.rule_templates[Number(pick)]?.rule;
              if (!rule) return;
              const added = addRule(ruleset, rule);
              edit(added.ruleset);
              onSelect(added.index);
            }}
          >
            <option value="">+ Add a rule…</option>
            <option value="blank">Blank rule (follows the trend)</option>
            {options.rule_templates.map((t, i) => (
              <option key={t.rule.name} value={i}>
                {t.rule.name} — {t.hint}
              </option>
            ))}
          </select>
        )}
      </div>

      <details className="rule-details">
        <summary>Shop-wide limits</summary>
        <GuardrailsEditor
          guardrails={ruleset.guardrails}
          options={options}
          onChange={edit ? (g) => edit({ ...ruleset, guardrails: g }) : null}
        />
      </details>
      <details className="rule-details">
        <summary>Condition &amp; language factors</summary>
        <FactorsEditor ruleset={ruleset} options={options} onChange={edit} />
      </details>
    </section>
  );
}

// ── One rule ────────────────────────────────────────────────────────────────

function RuleCard({
  index,
  rule,
  view,
  open,
  count,
  options,
  onToggle,
  onShow,
  onChange,
  onMove,
  onDuplicate,
  onDelete,
}: {
  index: number;
  rule: Rule;
  view: RuleView | undefined;
  open: boolean;
  count: number;
  options: RepricingOptions;
  onToggle: () => void;
  onShow: () => void;
  onChange: ((rule: Rule) => void) | null;
  onMove: ((step: -1 | 1) => void) | null;
  onDuplicate: (() => void) | null;
  onDelete: (() => void) | null;
}) {
  const classes = ["rule-card"];
  if (open) classes.push("rule-card--open");
  if (!rule.enabled) classes.push("rule-card--off");
  // The summary is only right while the server's view is of this rule.
  const current = view && view.name === rule.name ? view : undefined;
  return (
    <li className={classes.join(" ")}>
      <div className="rule-card__head">
        <span className="rule-card__number" aria-hidden="true">
          {index + 1}
        </span>
        <button type="button" className="rule-card__title" aria-expanded={open} onClick={onToggle}>
          <span className="rule-card__name">
            {rule.name || "Unnamed rule"}
            {!rule.enabled && <span className="badge">off</span>}
          </span>
          {current && (
            <span className="rule-card__summary">
              {current.when} → {current.then}
            </span>
          )}
        </button>
        <label className="switch" title={rule.enabled ? "Turn this rule off" : "Turn this rule on"}>
          <input
            type="checkbox"
            checked={rule.enabled}
            disabled={!onChange}
            onChange={(e) => onChange?.({ ...rule, enabled: e.target.checked })}
          />
          <span className="visually-hidden">Rule {rule.name} on</span>
        </label>
      </div>
      {current && current.matched > 0 && (
        <p className="rule-card__stats">
          <button type="button" className="link-button" onClick={onShow}>
            {formatCount(current.matched)} {current.matched === 1 ? "listing" : "listings"}
          </button>
          {current.raised > 0 && <span className="change--up"> · ↑ {formatCount(current.raised)}</span>}
          {current.lowered > 0 && <span className="change--down"> · ↓ {formatCount(current.lowered)}</span>}
          {(current.raised > 0 || current.lowered > 0) && <span> · {signedEuro(current.value_delta)}</span>}
        </p>
      )}
      {open && (
        <fieldset className="rule-editor" disabled={!onChange}>
          <legend className="visually-hidden">Rule {index + 1}</legend>
          <RuleEditor rule={rule} options={options} onChange={(r) => onChange?.(r)} />
          {onChange && (
            <div className="rule-editor__actions">
              <button
                type="button"
                className="button button--small"
                disabled={index === 0}
                onClick={() => onMove?.(-1)}
              >
                ⏶ Up
              </button>
              <button
                type="button"
                className="button button--small"
                disabled={index === count - 1}
                onClick={() => onMove?.(1)}
              >
                ⏷ Down
              </button>
              <button type="button" className="button button--small" onClick={() => onDuplicate?.()}>
                Duplicate
              </button>
              <button type="button" className="button button--small button--danger" onClick={() => onDelete?.()}>
                Delete
              </button>
            </div>
          )}
        </fieldset>
      )}
    </li>
  );
}

function RuleEditor({
  rule,
  options,
  onChange,
}: {
  rule: Rule;
  options: RepricingOptions;
  onChange: (rule: Rule) => void;
}) {
  const id = useId();
  const free = freeTemplates(rule, options.condition_templates);
  const setCondition = (k: number, c: RuleCondition) =>
    onChange({ ...rule, conditions: rule.conditions.map((x, i) => (i === k ? c : x)) });
  return (
    <>
      <Field label="Name" htmlFor={`${id}-name`}>
        <input
          id={`${id}-name`}
          className="rule-select"
          maxLength={80}
          value={rule.name}
          onChange={(e) => onChange({ ...rule, name: e.target.value })}
        />
      </Field>
      <RuleGroup title="When">
        {rule.conditions.length === 0 && <p className="muted small">Every listing (no conditions).</p>}
        {rule.conditions.map((c, k) => {
          const template = templateOf(c, options.condition_templates);
          const label = template?.label ?? conditionKind(c);
          return (
            <div className="condition" key={conditionKind(c)}>
              <div className="condition__head">
                <span className="rule-field__label" title={template?.help}>
                  {label}
                </span>
                <button
                  type="button"
                  className="button button--ghost button--small"
                  aria-label={`Remove the condition ${label}`}
                  onClick={() => onChange({ ...rule, conditions: rule.conditions.filter((_, i) => i !== k) })}
                >
                  ✕
                </button>
              </div>
              <ConditionEditor
                id={`${id}-c${k}`}
                condition={c}
                options={options}
                onChange={(x) => setCondition(k, x)}
              />
              {template && <span className="rule-field__hint">{template.help}</span>}
            </div>
          );
        })}
        {free.length > 0 && (
          <select
            className="rule-select"
            aria-label="Add a condition"
            value=""
            onChange={(e) => {
              const t = free[Number(e.target.value)];
              if (t) onChange({ ...rule, conditions: [...rule.conditions, structuredClone(t.condition)] });
            }}
          >
            <option value="">+ Add a condition…</option>
            {free.map((t, i) => (
              <option key={t.label} value={i}>
                {t.label} — {t.help}
              </option>
            ))}
          </select>
        )}
      </RuleGroup>
      <RuleGroup title="Price">
        <PriceEditor
          id={id}
          action={rule.action}
          options={options}
          onChange={(a) => onChange({ ...rule, action: a })}
        />
      </RuleGroup>
    </>
  );
}

// ── Conditions ──────────────────────────────────────────────────────────────

/** Chips for a list condition: pressed = in the list; none pressed = any. */
function ListChips({
  offered,
  picked,
  onChange,
}: {
  offered: string[];
  picked: string[];
  onChange: (picked: string[]) => void;
}) {
  const lower = (v: string) => v.toLowerCase();
  const all = [...offered, ...picked.filter((p) => !offered.some((o) => lower(o) === lower(p)))];
  const has = (v: string) => picked.some((p) => lower(p) === lower(v));
  return (
    <>
      <div className="chip-row">
        {all.map((v) => (
          <Chip
            key={v}
            on={has(v)}
            onClick={() => onChange(has(v) ? picked.filter((p) => lower(p) !== lower(v)) : [...picked, v])}
          >
            {v}
          </Chip>
        ))}
      </div>
      {picked.length === 0 && <span className="rule-field__hint">None picked: any.</span>}
    </>
  );
}

function Range({
  id,
  min,
  max,
  unit,
  rules,
  onChange,
}: {
  id: string;
  min: number | null;
  max: number | null;
  unit: string;
  rules: { min: number; max: number; integer?: boolean };
  onChange: (min: number | null, max: number | null) => void;
}) {
  const optional = { ...rules, optional: true };
  return (
    <div className="rule-row">
      <NumField
        id={`${id}-min`}
        label="From"
        value={min}
        rules={optional}
        unit={unit}
        onChange={(v) => onChange(v, max)}
      />
      <NumField
        id={`${id}-max`}
        label="To"
        value={max}
        rules={optional}
        unit={unit}
        onChange={(v) => onChange(min, v)}
      />
    </div>
  );
}

function TextTerms({ id, value, onChange }: { id: string; value: string; onChange: (value: string) => void }) {
  return (
    <input
      id={id}
      className="rule-select"
      maxLength={300}
      placeholder="Words, comma-separated"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

function ConditionEditor({
  id,
  condition: c,
  options,
  onChange,
}: {
  id: string;
  condition: RuleCondition;
  options: RepricingOptions;
  onChange: (condition: RuleCondition) => void;
}) {
  if ("Rarity" in c)
    return <ListChips offered={options.rarities} picked={c.Rarity} onChange={(v) => onChange({ Rarity: v })} />;
  if ("CardCondition" in c)
    return (
      <ListChips
        offered={options.conditions}
        picked={c.CardCondition}
        onChange={(v) => onChange({ CardCondition: v })}
      />
    );
  if ("Language" in c)
    return <ListChips offered={options.languages} picked={c.Language} onChange={(v) => onChange({ Language: v })} />;
  if ("Finish" in c)
    return (
      <div className="segmented" role="group" aria-label="Finish">
        {options.finishes.map((f) => (
          <button
            key={f.key}
            type="button"
            className={c.Finish === f.key ? "segmented__button segmented__button--on" : "segmented__button"}
            aria-pressed={c.Finish === f.key}
            onClick={() => onChange({ Finish: f.key })}
          >
            {f.label}
          </button>
        ))}
      </div>
    );
  if ("ListedPrice" in c)
    return (
      <Range
        id={id}
        unit="€"
        rules={PRICE}
        min={c.ListedPrice.min}
        max={c.ListedPrice.max}
        onChange={(min, max) => onChange({ ListedPrice: { min, max } })}
      />
    );
  if ("MarketPrice" in c)
    return (
      <Range
        id={id}
        unit="€"
        rules={PRICE}
        min={c.MarketPrice.min}
        max={c.MarketPrice.max}
        onChange={(min, max) => onChange({ MarketPrice: { min, max } })}
      />
    );
  if ("Name" in c) return <TextTerms id={id} value={c.Name} onChange={(v) => onChange({ Name: v })} />;
  if ("Set" in c) return <TextTerms id={id} value={c.Set} onChange={(v) => onChange({ Set: v })} />;
  if ("Location" in c) return <TextTerms id={id} value={c.Location} onChange={(v) => onChange({ Location: v })} />;
  if ("AgeDays" in c)
    return (
      <Range
        id={id}
        unit="days"
        rules={DAYS}
        min={c.AgeDays.min}
        max={c.AgeDays.max}
        onChange={(min, max) => onChange({ AgeDays: { min, max } })}
      />
    );
  if ("NoSaleDays" in c)
    return (
      <NumField
        id={id}
        label="Days without a sale, at least"
        value={c.NoSaleDays}
        rules={DAYS}
        unit="days"
        onChange={(v) => onChange({ NoSaleDays: v ?? 0 })}
      />
    );
  if ("MinSold30d" in c)
    return (
      <NumField
        id={id}
        label={`Sold in ${options.sales_window_days} days, at least`}
        value={c.MinSold30d}
        rules={DAYS}
        unit={`sold in ${options.sales_window_days} days`}
        onChange={(v) => onChange({ MinSold30d: v ?? 0 })}
      />
    );
  if ("MinQuantity" in c)
    return (
      <NumField
        id={id}
        label="Copies in stock, at least"
        value={c.MinQuantity}
        rules={DAYS}
        unit="copies"
        onChange={(v) => onChange({ MinQuantity: v ?? 0 })}
      />
    );
  return (
    <select
      id={id}
      className="rule-select"
      aria-label="Market trend"
      value={c.Moving}
      onChange={(e) => {
        const move = options.moves.find((m) => m.key === e.target.value);
        if (move) onChange({ Moving: move.key });
      }}
    >
      {options.moves.map((m) => (
        <option key={m.key} value={m.key}>
          {m.label}
        </option>
      ))}
    </select>
  );
}

// ── Price ───────────────────────────────────────────────────────────────────

function FieldSelect({
  id,
  label,
  value,
  options,
  onChange,
}: {
  id: string;
  label: string;
  value: PriceField;
  options: RepricingOptions;
  onChange: (field: PriceField) => void;
}) {
  return (
    <select
      id={id}
      className="rule-select"
      aria-label={label}
      value={value}
      onChange={(e) => {
        const f = options.fields.find((x) => x.key === e.target.value);
        if (f) onChange(f.key);
      }}
    >
      {options.fields.map((f) => (
        <option key={f.key} value={f.key}>
          {f.label}
        </option>
      ))}
    </select>
  );
}

function PriceEditor({
  id,
  action: a,
  options,
  onChange,
}: {
  id: string;
  action: RulePrice;
  options: RepricingOptions;
  onChange: (action: RulePrice) => void;
}) {
  const kind = baseKind(a.base);
  const set = (change: Partial<RulePrice>) => onChange({ ...a, ...change });
  const market = typeof a.base === "object" && "Market" in a.base ? a.base.Market : null;
  const fixed = typeof a.base === "object" && "Fixed" in a.base ? a.base.Fixed : null;
  return (
    <>
      <Field label="Start from" htmlFor={`${id}-base`}>
        <select
          id={`${id}-base`}
          className="rule-select"
          value={kind}
          onChange={(e) => set({ base: baseOf(e.target.value as BaseKind, a.base) })}
        >
          {BASE_KINDS.map((b) => (
            <option key={b.key} value={b.key}>
              {b.label}
            </option>
          ))}
        </select>
      </Field>
      {market && (
        <div className="rule-row">
          <FieldSelect
            id={`${id}-field`}
            label="Market column"
            value={market.field}
            options={options}
            onChange={(field) => set({ base: { Market: { ...market, field } } })}
          />
          <select
            className="rule-select"
            aria-label="Second column"
            value={market.combine}
            onChange={(e) => {
              const c = options.combines.find((x) => x.key === e.target.value);
              if (c) set({ base: { Market: { ...market, combine: c.key } } });
            }}
          >
            {options.combines.map((c) => (
              <option key={c.key} value={c.key}>
                {c.label}
              </option>
            ))}
          </select>
          {market.combine !== "Only" && (
            <FieldSelect
              id={`${id}-other`}
              label="Other market column"
              value={market.other}
              options={options}
              onChange={(other) => set({ base: { Market: { ...market, other } } })}
            />
          )}
        </div>
      )}
      {fixed !== null && (
        <Field label="Fixed price" htmlFor={`${id}-fixed`}>
          <NumField
            id={`${id}-fixed`}
            value={fixed}
            rules={{ min: options.min_price, max: options.max_price }}
            unit="€"
            onChange={(v) => v !== null && set({ base: { Fixed: v } })}
          />
        </Field>
      )}
      {kind === "Keep" && (
        <p className="muted small">The price stays, and no later rule touches these listings (floors neither).</p>
      )}
      {kind !== "Keep" && (
        <>
          <div className="rule-row">
            <Field label="Times" htmlFor={`${id}-pct`}>
              <NumField
                id={`${id}-pct`}
                value={a.percent}
                rules={PERCENT}
                unit="%"
                onChange={(v) => v !== null && set({ percent: v })}
              />
            </Field>
            <Field label="Plus" htmlFor={`${id}-offset`} hint="Negative to undercut">
              <NumField
                id={`${id}-offset`}
                value={a.offset}
                rules={{ min: -options.max_price, max: options.max_price }}
                unit="€"
                onChange={(v) => v !== null && set({ offset: v })}
              />
            </Field>
          </div>
          {market && (
            <label className="switch">
              <input type="checkbox" checked={a.adjust} onChange={(e) => set({ adjust: e.target.checked })} />
              <span>Adjust for condition and language</span>
            </label>
          )}
          <Field label="Direction">
            <div className="segmented" role="group" aria-label="Direction">
              {options.directions.map((d) => (
                <button
                  key={d.key}
                  type="button"
                  className={a.direction === d.key ? "segmented__button segmented__button--on" : "segmented__button"}
                  aria-pressed={a.direction === d.key}
                  onClick={() => set({ direction: d.key })}
                >
                  {d.label}
                </button>
              ))}
            </div>
          </Field>
          <div className="rule-row">
            <Field label="At least" htmlFor={`${id}-min`}>
              <NumField
                id={`${id}-min`}
                value={a.min_price}
                rules={PRICE}
                unit="€"
                onChange={(v) => set({ min_price: v })}
              />
            </Field>
            <Field label="At most" htmlFor={`${id}-max`}>
              <NumField
                id={`${id}-max`}
                value={a.max_price}
                rules={PRICE}
                unit="€"
                onChange={(v) => set({ max_price: v })}
              />
            </Field>
          </div>
          <div className="rule-row">
            <Field label="Raise at most" htmlFor={`${id}-raise`} hint="Per run; empty = no limit">
              <NumField
                id={`${id}-raise`}
                value={a.max_raise_pct}
                rules={{ ...PERCENT, optional: true }}
                unit="%"
                onChange={(v) => set({ max_raise_pct: v })}
              />
            </Field>
            <Field label="Drop at most" htmlFor={`${id}-drop`} hint="Per run; empty = no limit">
              <NumField
                id={`${id}-drop`}
                value={a.max_drop_pct}
                rules={{ min: 0, max: 100, optional: true }}
                unit="%"
                onChange={(v) => set({ max_drop_pct: v })}
              />
            </Field>
          </div>
        </>
      )}
    </>
  );
}

// ── Shop-wide limits and factors ────────────────────────────────────────────

function GuardrailsEditor({
  guardrails: g,
  options,
  onChange,
}: {
  guardrails: Guardrails;
  options: RepricingOptions;
  onChange: ((g: Guardrails) => void) | null;
}) {
  const id = useId();
  const set = (change: Partial<Guardrails>) => onChange?.({ ...g, ...change });
  const floors = ["common", "uncommon", "rare", "mythic", "other"] as const;
  return (
    <fieldset className="rule-editor" disabled={!onChange}>
      <legend className="visually-hidden">Shop-wide limits</legend>
      <Section title="Lowest price per rarity" hint="Floors always win, even over “only lower” and the dead band.">
        <div className="rule-row rule-row--wide">
          {floors.map((f) => (
            <Field key={f} label={f[0]?.toUpperCase() + f.slice(1)} htmlFor={`${id}-${f}`}>
              <NumField
                id={`${id}-${f}`}
                value={g.floors[f]}
                rules={{ min: 0, max: options.max_price }}
                unit="€"
                onChange={(v) => v !== null && set({ floors: { ...g.floors, [f]: v } })}
              />
            </Field>
          ))}
        </div>
      </Section>
      <Field label="Rounding" htmlFor={`${id}-rounding`}>
        <select
          id={`${id}-rounding`}
          className="rule-select"
          value={g.rounding}
          onChange={(e) => {
            const r = options.roundings.find((x) => x.key === e.target.value);
            if (r) set({ rounding: r.key });
          }}
        >
          {options.roundings.map((r) => (
            <option key={r.key} value={r.key}>
              {r.label}
            </option>
          ))}
        </select>
      </Field>
      <Section title="Ignore changes below" hint="Both must be passed for a change to count.">
        <div className="rule-row">
          <NumField
            id={`${id}-min-eur`}
            label="Ignore changes below (€)"
            value={g.min_change_eur}
            rules={{ min: 0, max: 1000 }}
            unit="€"
            onChange={(v) => v !== null && set({ min_change_eur: v })}
          />
          <NumField
            id={`${id}-min-pct`}
            label="Ignore changes below (%)"
            value={g.min_change_pct}
            rules={{ min: 0, max: 100 }}
            unit="%"
            onChange={(v) => v !== null && set({ min_change_pct: v })}
          />
        </div>
      </Section>
      <Section title="Needs a tick above" hint="Bigger changes start unticked until you confirm them.">
        <div className="rule-row">
          <NumField
            id={`${id}-review-pct`}
            label="Needs a tick above (%)"
            value={g.review_pct}
            rules={PERCENT}
            unit="%"
            onChange={(v) => v !== null && set({ review_pct: v })}
          />
          <NumField
            id={`${id}-review-eur`}
            label="and at least (€)"
            value={g.review_min_eur}
            rules={{ min: 0, max: options.max_price }}
            unit="€"
            onChange={(v) => v !== null && set({ review_min_eur: v })}
          />
        </div>
      </Section>
      <div className="rule-row">
        <Field label="Skip market data older than" htmlFor={`${id}-stale`} hint="0 = always use it">
          <NumField
            id={`${id}-stale`}
            value={g.stale_days}
            rules={{ min: 0, max: 3650, integer: true }}
            unit="days"
            onChange={(v) => v !== null && set({ stale_days: v })}
          />
        </Field>
        <Field label="Reference column" htmlFor={`${id}-reference`} hint="For “market price” conditions and the table">
          <FieldSelect
            id={`${id}-reference`}
            label="Reference column"
            value={g.reference}
            options={options}
            onChange={(reference) => set({ reference })}
          />
        </Field>
      </div>
    </fieldset>
  );
}

function Section({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <div className="rule-field">
      <span className="rule-field__label">{title}</span>
      {children}
      {hint && <span className="rule-field__hint">{hint}</span>}
    </div>
  );
}

function FactorsEditor({
  ruleset,
  options,
  onChange,
}: {
  ruleset: Ruleset;
  options: RepricingOptions;
  onChange: ((ruleset: Ruleset) => void) | null;
}) {
  const id = useId();
  const a = ruleset.adjustments;
  // Stored keys may differ in case from the stock's spelling; keep using them.
  const languageKey = (lang: string) =>
    Object.keys(a.language_pct).find((k) => k.toLowerCase() === lang.toLowerCase()) ?? lang;
  const languageValue = (lang: string) => a.language_pct[languageKey(lang)] ?? 100;
  return (
    <fieldset className="rule-editor" disabled={!onChange}>
      <legend className="visually-hidden">Condition and language factors</legend>
      <p className="muted small">
        Market prices are about near-mint and language-blind; rules with “adjust” scale them by these factors.
      </p>
      <Section title="Condition">
        <div className="rule-row rule-row--wide">
          {options.conditions.map((c) => (
            <Field key={c} label={c} htmlFor={`${id}-c-${c}`}>
              <NumField
                id={`${id}-c-${c}`}
                value={a.condition_pct[c] ?? 100}
                rules={PERCENT}
                unit="%"
                onChange={(v) =>
                  v !== null &&
                  onChange?.({ ...ruleset, adjustments: { ...a, condition_pct: { ...a.condition_pct, [c]: v } } })
                }
              />
            </Field>
          ))}
        </div>
      </Section>
      <Section title="Language">
        <div className="rule-row rule-row--wide">
          {options.languages.map((lang) => (
            <Field key={lang} label={lang} htmlFor={`${id}-l-${lang}`}>
              <NumField
                id={`${id}-l-${lang}`}
                value={languageValue(lang)}
                rules={PERCENT}
                unit="%"
                onChange={(v) =>
                  v !== null &&
                  onChange?.({
                    ...ruleset,
                    adjustments: { ...a, language_pct: { ...a.language_pct, [languageKey(lang)]: v } },
                  })
                }
              />
            </Field>
          ))}
        </div>
      </Section>
    </fieldset>
  );
}
